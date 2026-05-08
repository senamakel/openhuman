import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { type FC, useEffect, useRef, useState } from 'react';

import { YellowMascot } from '../human/Mascot/YellowMascot';

/**
 * Meet camera frame producer.
 *
 * Mounted once at app root. Listens for the shell-emitted
 * `meet-video:bus-started` / `meet-video:bus-stopped` events and, while
 * a session is active, renders a hidden Remotion-driven mascot,
 * rasterizes its SVG to a 640×480 JPEG every frame, and pushes the
 * bytes over a loopback WebSocket to the Rust frame bus
 * (`app/src-tauri/src/meet_video/frame_bus.rs`). The Rust side fans
 * each frame out to the consumer — the camera bridge inside the Meet
 * CEF webview, which paints them onto its capture canvas
 * (`canvas.captureStream(30)` → `getUserMedia` intercept).
 *
 * ## Why the mascot lives here, not in the Meet webview
 *
 * `CLAUDE.md` rules out growing JS injection into CEF child webviews.
 * The Remotion runtime + composition tree is too large to inject and
 * would run inside a third-party origin sandbox; that's a non-starter.
 * Instead the rich animation lives in our own renderer (where Remotion
 * is already a project dependency) and we ship its pixels — not its
 * code — to the Meet origin.
 *
 * ## Why XMLSerializer instead of `@remotion/player`
 *
 * Remotion's `<Player>` historically failed to start cold inside CEF
 * (see `app/src/features/human/Mascot/yellow/frameContext.tsx`); the
 * project replaced it with a local `FrameProvider` that drives ticks
 * via `requestAnimationFrame`. The compositions render to live SVG,
 * which we rasterize per frame: serialize → data URI → `<img>` decode
 * → drawImage → JPEG blob.
 */

const PRODUCER_FPS = 24; // 24 fps is plenty for "lifelike" and gives
// per-frame serialize+encode budget headroom — at 30 fps the SVG decode
// occasionally backs up on slower machines and frames pile up. The
// bridge consumer redraws its canvas at 30 fps regardless, repeating
// our latest frame between producer ticks.

const FRAME_W = 640;
const FRAME_H = 480;
const JPEG_QUALITY = 0.72;

interface BusSession {
  requestId: string;
  port: number;
}

export const MascotFrameProducer: FC = () => {
  const [session, setSession] = useState<BusSession | null>(null);

  useEffect(() => {
    let unlistenStarted: UnlistenFn | undefined;
    let unlistenStopped: UnlistenFn | undefined;
    let cancelled = false;

    listen<BusSession>('meet-video:bus-started', event => {
      const payload = event.payload;
      if (!payload || !payload.port) return;
      console.log('[meet-video-producer] bus-started', payload);
      setSession(payload);
    })
      .then(stop => {
        if (cancelled) stop();
        else unlistenStarted = stop;
      })
      .catch(() => {});

    listen<{ requestId?: string; request_id?: string }>('meet-video:bus-stopped', event => {
      console.log('[meet-video-producer] bus-stopped', event.payload);
      setSession(null);
    })
      .then(stop => {
        if (cancelled) stop();
        else unlistenStopped = stop;
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      if (unlistenStarted) unlistenStarted();
      if (unlistenStopped) unlistenStopped();
    };
  }, []);

  if (!session) return null;
  return <ProducerSession key={session.requestId} session={session} />;
};

const ProducerSession: FC<{ session: BusSession }> = ({ session }) => {
  const hostRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const wsReadyRef = useRef(false);
  const stoppedRef = useRef(false);
  const inflightRef = useRef(false);
  const sentFramesRef = useRef(0);
  const lastLogRef = useRef(0);

  useEffect(() => {
    stoppedRef.current = false;

    // ── WS connect ─────────────────────────────────────────────────────
    const url = `ws://127.0.0.1:${session.port}`;
    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch (err) {
      console.warn('[meet-video-producer] ws ctor failed', err);
      return;
    }
    ws.binaryType = 'arraybuffer';
    wsRef.current = ws;
    ws.onopen = () => {
      wsReadyRef.current = true;
      console.log('[meet-video-producer] ws connected', url);
    };
    ws.onclose = () => {
      wsReadyRef.current = false;
      console.log('[meet-video-producer] ws closed');
    };
    ws.onerror = err => {
      console.warn('[meet-video-producer] ws error', err);
    };

    // ── Per-frame rasterize + push loop ───────────────────────────────
    // Reused across ticks. The OffscreenCanvas keeps the JPEG encode off
    // the main DOM canvas pipeline.
    const offscreen =
      typeof OffscreenCanvas !== 'undefined'
        ? new OffscreenCanvas(FRAME_W, FRAME_H)
        : (() => {
            const c = document.createElement('canvas');
            c.width = FRAME_W;
            c.height = FRAME_H;
            return c as unknown as OffscreenCanvas;
          })();
    const ctx = (offscreen as unknown as OffscreenCanvas).getContext(
      '2d'
    ) as OffscreenCanvasRenderingContext2D | null;
    if (!ctx) {
      console.warn('[meet-video-producer] no 2d ctx — aborting');
      return;
    }
    const serializer = typeof XMLSerializer !== 'undefined' ? new XMLSerializer() : null;

    const intervalMs = Math.round(1000 / PRODUCER_FPS);
    const timer = window.setInterval(() => {
      if (stoppedRef.current || !wsReadyRef.current) return;
      // Drop frames if a previous encode is still inflight rather than
      // letting them queue up unbounded.
      if (inflightRef.current) return;
      const host = hostRef.current;
      if (!host || !serializer) return;
      const svg = host.querySelector('svg');
      if (!svg) return;
      inflightRef.current = true;
      void encodeAndSend(svg, serializer, ctx, ws)
        .then(ok => {
          if (ok) {
            sentFramesRef.current++;
            const now = Date.now();
            if (now - lastLogRef.current > 5000) {
              lastLogRef.current = now;
              console.log(`[meet-video-producer] sent_total=${sentFramesRef.current}`);
            }
          }
        })
        .finally(() => {
          inflightRef.current = false;
        });
    }, intervalMs);

    return () => {
      stoppedRef.current = true;
      window.clearInterval(timer);
      try {
        ws.close();
      } catch (err) {
        console.debug('[meet-video-producer] ws close failed', err);
      }
      wsRef.current = null;
      wsReadyRef.current = false;
    };
  }, [session.port]);

  // The mascot host lives off-screen but in the layout tree so the SVG
  // gets laid out + animated normally. Fixed pixel size so the SVG
  // serialization renders at a predictable resolution.
  return (
    <div
      ref={hostRef}
      aria-hidden="true"
      style={{
        position: 'fixed',
        left: '-99999px',
        top: 0,
        width: FRAME_H,
        height: FRAME_H,
        pointerEvents: 'none',
        opacity: 0,
      }}>
      <YellowMascot face="idle" arm="wave" size={FRAME_H} />
    </div>
  );
};

async function encodeAndSend(
  svg: SVGElement,
  serializer: XMLSerializer,
  ctx: OffscreenCanvasRenderingContext2D,
  ws: WebSocket
): Promise<boolean> {
  try {
    // Make sure the SVG carries width/height/xmlns so the standalone
    // data URI parses on its own (it's pulled out of the React tree).
    const clone = svg.cloneNode(true) as SVGElement;
    if (!clone.hasAttribute('xmlns')) {
      clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
    }
    if (!clone.hasAttribute('width')) clone.setAttribute('width', `${FRAME_H}`);
    if (!clone.hasAttribute('height')) clone.setAttribute('height', `${FRAME_H}`);
    const xml = serializer.serializeToString(clone);
    const url = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(xml);

    const img = new window.Image();
    img.decoding = 'async';
    img.src = url;
    await img.decode();

    // Background — matches the bridge's fill so producer/fallback
    // transitions don't flash.
    ctx.fillStyle = '#F7F4EE';
    ctx.fillRect(0, 0, FRAME_W, FRAME_H);
    // cover-fit the square mascot into the 640×480 frame.
    const scale = Math.max(FRAME_W / img.naturalWidth, FRAME_H / img.naturalHeight);
    const dw = img.naturalWidth * scale;
    const dh = img.naturalHeight * scale;
    const dx = (FRAME_W - dw) / 2;
    const dy = (FRAME_H - dh) / 2;
    ctx.drawImage(img, dx, dy, dw, dh);

    const oc = ctx.canvas as OffscreenCanvas;
    const blob =
      'convertToBlob' in oc
        ? await oc.convertToBlob({ type: 'image/jpeg', quality: JPEG_QUALITY })
        : await new Promise<Blob>((resolve, reject) => {
            (ctx.canvas as unknown as HTMLCanvasElement).toBlob(
              b => (b ? resolve(b) : reject(new Error('toBlob null'))),
              'image/jpeg',
              JPEG_QUALITY
            );
          });
    const buffer = await blob.arrayBuffer();
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(buffer);
      return true;
    }
    return false;
  } catch (err) {
    console.warn('[meet-video-producer] encode/send failed', err);
    return false;
  }
}

export default MascotFrameProducer;
