import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import RotatingTetrahedronCanvas from '../components/RotatingTetrahedronCanvas';
import { callParentCoreRpc } from './parentCoreRpc';

const TARGET_SAMPLE_RATE = 16000;
const OVERLAY_WIDTH = 240;
const OVERLAY_HEIGHT = 220;

type OverlayStatus = 'idle' | 'listening' | 'transcribing' | 'ready' | 'error';

interface TranscribeResult {
  text: string;
  raw_text: string;
  model_id: string;
}

interface GlobeHotkeyStatus {
  supported: boolean;
  running: boolean;
  input_monitoring_permission: string;
  last_error: string | null;
  events_pending: number;
}

interface GlobeHotkeyPollResult {
  status: GlobeHotkeyStatus;
  events: string[];
}

interface AppContextInfo {
  app_name: string | null;
  window_title: string | null;
}

interface AccessibilitySessionStatus {
  active: boolean;
  capture_count: number;
  frames_in_memory: number;
  last_capture_at_ms: number | null;
  last_context: string | null;
  last_window_title: string | null;
  vision_enabled: boolean;
  vision_state: string;
  vision_queue_depth: number;
}

interface AccessibilityStatus {
  is_context_blocked: boolean;
  foreground_context: AppContextInfo | null;
  session: AccessibilitySessionStatus;
}

interface AutocompleteSuggestion {
  value: string;
  confidence: number;
}

interface AutocompleteStatus {
  platform_supported: boolean;
  enabled: boolean;
  running: boolean;
  phase: string;
  app_name: string | null;
  last_error: string | null;
  updated_at_ms: number | null;
  suggestion: AutocompleteSuggestion | null;
}

interface VoiceStatus {
  stt_available: boolean;
  tts_available: boolean;
  stt_model_id: string;
  tts_voice_id: string;
  whisper_binary: string | null;
  piper_binary: string | null;
  stt_model_path: string | null;
  tts_voice_path: string | null;
  whisper_in_process: boolean;
  llm_cleanup_enabled: boolean;
}

interface OverlayDebugSnapshot {
  screen: AccessibilityStatus | null;
  autocomplete: AutocompleteStatus | null;
  voice: VoiceStatus | null;
  updatedAt: number | null;
  error: string | null;
}

interface OverlayBubble {
  id: string;
  text: string;
  tone: 'neutral' | 'accent' | 'success' | 'warning' | 'danger';
  compact?: boolean;
}

function logOverlay(message: string, details?: unknown) {
  if (details) {
    console.debug(`[overlay] ${message}`, details);
    return;
  }
  console.debug(`[overlay] ${message}`);
}

function floatTo16BitPCM(output: DataView, offset: number, input: Float32Array) {
  for (let i = 0; i < input.length; i += 1, offset += 2) {
    const sample = Math.max(-1, Math.min(1, input[i]));
    output.setInt16(offset, sample < 0 ? sample * 0x8000 : sample * 0x7fff, true);
  }
}

function encodeWavMono16k(samples: Float32Array, sampleRate: number): Uint8Array {
  const bytesPerSample = 2;
  const blockAlign = bytesPerSample;
  const byteRate = sampleRate * blockAlign;
  const dataSize = samples.length * bytesPerSample;
  const buffer = new ArrayBuffer(44 + dataSize);
  const view = new DataView(buffer);

  const writeString = (offset: number, value: string) => {
    for (let i = 0; i < value.length; i += 1) {
      view.setUint8(offset + i, value.charCodeAt(i));
    }
  };

  writeString(0, 'RIFF');
  view.setUint32(4, 36 + dataSize, true);
  writeString(8, 'WAVE');
  writeString(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, byteRate, true);
  view.setUint16(32, blockAlign, true);
  view.setUint16(34, 16, true);
  writeString(36, 'data');
  view.setUint32(40, dataSize, true);
  floatTo16BitPCM(view, 44, samples);

  return new Uint8Array(buffer);
}

async function toMono16k(audioBuffer: AudioBuffer): Promise<Float32Array> {
  const channels = audioBuffer.numberOfChannels;
  const mono = new Float32Array(audioBuffer.length);

  for (let c = 0; c < channels; c += 1) {
    const channelData = audioBuffer.getChannelData(c);
    for (let i = 0; i < audioBuffer.length; i += 1) {
      mono[i] += channelData[i] / channels;
    }
  }

  if (audioBuffer.sampleRate === TARGET_SAMPLE_RATE) {
    return mono;
  }

  const targetLength = Math.max(
    1,
    Math.round((mono.length * TARGET_SAMPLE_RATE) / audioBuffer.sampleRate)
  );
  const offline = new OfflineAudioContext(1, targetLength, TARGET_SAMPLE_RATE);
  const sourceBuffer = offline.createBuffer(1, mono.length, audioBuffer.sampleRate);
  sourceBuffer.copyToChannel(mono, 0);
  const source = offline.createBufferSource();
  source.buffer = sourceBuffer;
  source.connect(offline.destination);
  source.start();
  const rendered = await offline.startRendering();
  return rendered.getChannelData(0).slice();
}

async function convertBlobToWavBytes(blob: Blob): Promise<number[]> {
  const arrayBuffer = await blob.arrayBuffer();
  const audioContext = new AudioContext();

  try {
    const decoded = await audioContext.decodeAudioData(arrayBuffer.slice(0));
    const mono16k = await toMono16k(decoded);
    return Array.from(encodeWavMono16k(mono16k, TARGET_SAMPLE_RATE));
  } finally {
    await audioContext.close();
  }
}

function bubbleToneClass(tone: OverlayBubble['tone']) {
  switch (tone) {
    case 'accent':
      return 'border-sky-300/45 bg-sky-400/14 text-sky-50';
    case 'success':
      return 'border-emerald-300/45 bg-emerald-400/14 text-emerald-50';
    case 'warning':
      return 'border-amber-300/45 bg-amber-400/14 text-amber-50';
    case 'danger':
      return 'border-rose-300/45 bg-rose-400/14 text-rose-50';
    default:
      return 'border-white/18 bg-white/10 text-white';
  }
}

function OverlayBubbleChip({ bubble }: { bubble: OverlayBubble }) {
  return (
    <div
      className={`max-w-[184px] rounded-[18px] border px-3 py-2 text-left shadow-[0_18px_40px_rgba(3,7,18,0.28)] backdrop-blur-xl transition-all duration-200 ${bubbleToneClass(bubble.tone)} ${bubble.compact ? 'text-[10px] leading-4' : 'text-[11px] leading-[1.35]'}`}>
      {bubble.text}
    </div>
  );
}

export default function OverlayApp() {
  const appWindow = getCurrentWindow();
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const sessionIdRef = useRef(0);
  const globePollInFlightRef = useRef(false);

  const [parentRpcUrl, setParentRpcUrl] = useState<string | null | undefined>(undefined);
  const [coreReachable, setCoreReachable] = useState(true);
  const [voiceCaptureEnabled, setVoiceCaptureEnabled] = useState(true);
  const [status, setStatus] = useState<OverlayStatus>('idle');
  const [message, setMessage] = useState('Tap to talk');
  const [transcript, setTranscript] = useState('');
  const [debugSnapshot, setDebugSnapshot] = useState<OverlayDebugSnapshot>({
    screen: null,
    autocomplete: null,
    voice: null,
    updatedAt: null,
    error: null,
  });

  useEffect(() => {
    const size = new LogicalSize(OVERLAY_WIDTH, OVERLAY_HEIGHT);
    void appWindow.setSize(size).catch(error => {
      console.warn('[overlay] failed to resize overlay window', error);
    });
    void appWindow.setMinSize(size).catch(error => {
      console.warn('[overlay] failed to set overlay min size', error);
    });
    void appWindow.setMaxSize(size).catch(error => {
      console.warn('[overlay] failed to set overlay max size', error);
    });
  }, [appWindow]);

  useEffect(() => {
    let mounted = true;
    void invoke<string | null>('overlay_parent_rpc_url')
      .then(url => {
        if (!mounted) return;
        const trimmed = url?.trim();
        setParentRpcUrl(trimmed && trimmed.length > 0 ? trimmed : null);
      })
      .catch(error => {
        console.warn('[overlay] failed to resolve parent RPC URL', error);
        if (mounted) {
          setParentRpcUrl(null);
        }
      });
    return () => {
      mounted = false;
    };
  }, []);

  const rpc = useCallback(
    async <T,>(method: string, params: Record<string, unknown> = {}): Promise<T> => {
      if (parentRpcUrl === undefined) {
        throw new Error('[overlay] RPC not initialized');
      }
      if (!parentRpcUrl) {
        throw new Error('[overlay] core RPC URL unavailable');
      }
      return callParentCoreRpc<T>(parentRpcUrl, method, params);
    },
    [parentRpcUrl]
  );

  useEffect(() => {
    let disposed = false;

    const showOverlayFallback = async (nextMessage: string) => {
      if (disposed) {
        return;
      }
      logOverlay('globe listener unavailable', { message: nextMessage });
      setMessage(nextMessage);
      await appWindow.show().catch(() => {});
    };

    const startGlobeListener = async () => {
      if (parentRpcUrl === undefined) {
        return;
      }
      try {
        const result = await rpc<GlobeHotkeyStatus>(
          'openhuman.screen_intelligence_globe_listener_start',
          {}
        );
        logOverlay('globe listener start result', result);

        if (!result.supported) {
          await showOverlayFallback('Globe/Fn is only supported on macOS');
          return;
        }

        if (!result.running) {
          await showOverlayFallback(
            result.last_error ?? 'Globe/Fn listener could not start. Check Input Monitoring.'
          );
        }
      } catch (error) {
        console.error('[overlay] failed to start globe listener', error);
        await showOverlayFallback('Failed to start Globe/Fn listener');
      }
    };

    const pollGlobeListener = async () => {
      if (disposed || parentRpcUrl === undefined || globePollInFlightRef.current) {
        return;
      }
      globePollInFlightRef.current = true;

      try {
        const result = await rpc<GlobeHotkeyPollResult>(
          'openhuman.screen_intelligence_globe_listener_poll',
          {}
        );

        if (disposed) {
          return;
        }

        if (!result.status.running && result.status.last_error) {
          setMessage(result.status.last_error);
        }

        if (result.events.includes('FN_UP')) {
          const visible = await appWindow.isVisible();
          logOverlay('received FN_UP', { visible });
          if (visible) {
            await appWindow.hide();
          } else {
            await appWindow.show();
          }
        }
      } catch (error) {
        if (!disposed) {
          console.warn('[overlay] globe listener poll failed', error);
        }
      } finally {
        globePollInFlightRef.current = false;
      }
    };

    void startGlobeListener();
    const intervalId = window.setInterval(() => {
      void pollGlobeListener();
    }, 175);

    return () => {
      disposed = true;
      window.clearInterval(intervalId);
      if (parentRpcUrl === undefined) {
        return;
      }
      void rpc('openhuman.screen_intelligence_globe_listener_stop', {}).catch(() => {});
    };
  }, [appWindow, parentRpcUrl, rpc]);

  useEffect(() => {
    let disposed = false;
    let pollInFlight = false;

    const pollDebugState = async () => {
      if (disposed || parentRpcUrl === undefined || pollInFlight) {
        return;
      }
      pollInFlight = true;

      try {
        try {
          await rpc<{ ok?: boolean }>('core.ping', {});
          if (!disposed) {
            setCoreReachable(true);
          }
        } catch {
          if (!disposed) {
            setCoreReachable(false);
          }
        }

        const [screen, autocomplete, voice] = await Promise.all([
          rpc<AccessibilityStatus>('openhuman.screen_intelligence_status', {}),
          rpc<AutocompleteStatus>('openhuman.autocomplete_status', {}),
          rpc<VoiceStatus>('openhuman.voice_status', {}),
        ]);

        if (disposed) {
          return;
        }

        logOverlay('overlay state refreshed', {
          screenActive: screen.session.active,
          autocompletePhase: autocomplete.phase,
          hasSuggestion: Boolean(autocomplete.suggestion?.value),
          sttAvailable: voice.stt_available,
        });

        setDebugSnapshot({ screen, autocomplete, voice, updatedAt: Date.now(), error: null });
      } catch (error) {
        if (disposed) {
          return;
        }

        const nextError =
          error instanceof Error ? error.message : 'Failed to refresh overlay state';
        console.warn('[overlay] state poll failed', error);
        setDebugSnapshot(previous => ({ ...previous, updatedAt: Date.now(), error: nextError }));
      } finally {
        pollInFlight = false;
      }
    };

    void pollDebugState();
    const intervalId = window.setInterval(() => {
      void pollDebugState();
    }, 900);

    return () => {
      disposed = true;
      window.clearInterval(intervalId);
    };
  }, [parentRpcUrl, rpc]);

  const insertTranscriptIntoFocusedField = useCallback(
    async (text: string) => {
      logOverlay('inserting transcript into focused field', { length: text.length });
      await appWindow.hide();
      await new Promise(resolve => window.setTimeout(resolve, 120));

      try {
        const result = await rpc<{ inserted: boolean; error: string | null }>(
          'openhuman.text_input_insert_text',
          { text }
        );
        if (!result.inserted) {
          throw new Error(result.error ?? 'Text insert failed');
        }
        logOverlay('transcript inserted via text_input RPC');
      } catch (error) {
        console.warn('[overlay] insert failed, falling back to clipboard', error);
        await navigator.clipboard.writeText(text);
      }
    },
    [appWindow, rpc]
  );

  const cleanupStream = useCallback(() => {
    streamRef.current?.getTracks().forEach(track => track.stop());
    streamRef.current = null;
  }, []);

  const resetForNextCapture = useCallback(() => {
    setTranscript('');
    setStatus('idle');
    setMessage('Tap to talk');
  }, []);

  const transcribeBlob = useCallback(
    async (blob: Blob, sessionId: number) => {
      try {
        const audioBytes = await convertBlobToWavBytes(blob);
        const result = await rpc<TranscribeResult>('openhuman.voice_transcribe_bytes', {
          audio_bytes: audioBytes,
          extension: 'wav',
          skip_cleanup: false,
        });

        if (sessionIdRef.current !== sessionId) {
          return;
        }

        const nextTranscript = result.text.trim();
        if (!nextTranscript) {
          setTranscript('');
          setStatus('error');
          setMessage('No speech detected');
          return;
        }

        setTranscript(nextTranscript);
        setStatus('ready');
        setMessage('Dropping that into the active field');
        await insertTranscriptIntoFocusedField(nextTranscript);
        if (sessionIdRef.current !== sessionId) {
          return;
        }
        setMessage('Sent');
        window.setTimeout(() => {
          if (sessionIdRef.current === sessionId) {
            resetForNextCapture();
          }
        }, 1200);
      } catch (error) {
        if (sessionIdRef.current !== sessionId) {
          return;
        }

        console.error('[overlay] transcription failed', error);
        setTranscript('');
        setStatus('error');
        setMessage(error instanceof Error ? error.message : 'Transcription failed');
      }
    },
    [insertTranscriptIntoFocusedField, resetForNextCapture, rpc]
  );

  const stopRecording = useCallback(() => {
    if (!mediaRecorderRef.current || mediaRecorderRef.current.state === 'inactive') {
      return;
    }

    setStatus('transcribing');
    setMessage('Transcribing...');
    mediaRecorderRef.current.stop();
    mediaRecorderRef.current = null;
  }, []);

  const startRecording = useCallback(async () => {
    const nextSessionId = sessionIdRef.current + 1;
    sessionIdRef.current = nextSessionId;
    setTranscript('');
    setStatus('listening');
    setMessage('Listening...');
    chunksRef.current = [];

    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      streamRef.current = stream;

      const mimeType = MediaRecorder.isTypeSupported('audio/webm;codecs=opus')
        ? 'audio/webm;codecs=opus'
        : MediaRecorder.isTypeSupported('audio/webm')
          ? 'audio/webm'
          : 'audio/ogg';

      const recorder = new MediaRecorder(stream, { mimeType });
      mediaRecorderRef.current = recorder;

      recorder.ondataavailable = event => {
        if (event.data.size > 0) {
          chunksRef.current.push(event.data);
        }
      };

      recorder.onerror = event => {
        console.error('[overlay] media recorder error', event);
        cleanupStream();
        setStatus('error');
        setMessage('Microphone recording failed');
      };

      recorder.onstop = () => {
        cleanupStream();
        const blob = new Blob(chunksRef.current, { type: mimeType });
        chunksRef.current = [];

        if (blob.size === 0) {
          setStatus('error');
          setMessage('No audio recorded');
          return;
        }

        logOverlay('recording stopped, starting transcription', { blobSize: blob.size, mimeType });
        void transcribeBlob(blob, nextSessionId);
      };

      logOverlay('recording started', { mimeType });
      recorder.start(100);
    } catch (error) {
      console.error('[overlay] getUserMedia failed', error);
      cleanupStream();
      setStatus('error');
      setMessage(error instanceof Error ? error.message : 'Microphone access failed');
    }
  }, [cleanupStream, transcribeBlob]);

  const handleMainButton = useCallback(() => {
    if (status === 'listening') {
      logOverlay('main button toggled to stop listening');
      stopRecording();
      return;
    }

    if (!voiceCaptureEnabled) {
      setMessage('Voice capture is paused');
      return;
    }
    if (debugSnapshot.voice && !debugSnapshot.voice.stt_available) {
      setMessage('Speech-to-text is unavailable');
      return;
    }

    logOverlay('main button toggled to start listening', { priorStatus: status });
    void startRecording();
  }, [debugSnapshot.voice, startRecording, status, stopRecording, voiceCaptureEnabled]);

  const activeScreenApp =
    debugSnapshot.screen?.foreground_context?.app_name ??
    debugSnapshot.screen?.session.last_context ??
    'Current app';
  const autocompleteSuggestion = debugSnapshot.autocomplete?.suggestion?.value?.trim() ?? '';
  const sttAvailable = debugSnapshot.voice?.stt_available ?? true;
  const waitingForCoreConfig = parentRpcUrl === undefined;
  const voiceBlocked =
    !voiceCaptureEnabled || (debugSnapshot.voice !== null && !debugSnapshot.voice.stt_available);
  const orbDisabled =
    waitingForCoreConfig || (voiceCaptureEnabled && debugSnapshot.voice !== null && !sttAvailable);

  const bubbles = useMemo<OverlayBubble[]>(() => {
    const items: OverlayBubble[] = [];

    if (waitingForCoreConfig) {
      items.push({
        id: 'boot',
        text: 'Connecting to OpenHuman...',
        tone: 'neutral',
        compact: true,
      });
      return items;
    }

    if (!coreReachable) {
      items.push({
        id: 'core',
        text: 'Core offline. The orb is awake but the brain is not responding.',
        tone: 'danger',
      });
    }

    if (debugSnapshot.error) {
      items.push({ id: 'error', text: debugSnapshot.error, tone: 'danger' });
    } else if (status === 'listening') {
      items.push({ id: 'status', text: 'Listening...', tone: 'accent' });
    } else if (status === 'transcribing') {
      items.push({ id: 'status', text: 'Transcribing what you just said...', tone: 'warning' });
    } else if (status === 'ready') {
      items.push({ id: 'status', text: 'Sent to the active field.', tone: 'success' });
    } else if (status === 'error') {
      items.push({ id: 'status', text: message, tone: 'danger' });
    } else {
      items.push({
        id: 'hint',
        text: voiceCaptureEnabled ? `Ready in ${activeScreenApp}` : 'Voice capture paused',
        tone: 'neutral',
        compact: true,
      });
    }

    if (transcript) {
      items.push({ id: 'transcript', text: transcript, tone: 'success' });
    } else if (autocompleteSuggestion && status === 'idle') {
      items.push({ id: 'suggestion', text: autocompleteSuggestion, tone: 'accent' });
    } else if (!sttAvailable) {
      items.push({
        id: 'voice-unavailable',
        text: 'Speech-to-text is not configured yet.',
        tone: 'warning',
        compact: true,
      });
    }

    items.push({
      id: 'control',
      text: voiceCaptureEnabled ? 'Voice on' : 'Voice off',
      tone: voiceCaptureEnabled ? 'success' : 'neutral',
      compact: true,
    });

    return items.slice(0, 3);
  }, [
    activeScreenApp,
    autocompleteSuggestion,
    coreReachable,
    debugSnapshot.error,
    message,
    status,
    sttAvailable,
    transcript,
    voiceCaptureEnabled,
    waitingForCoreConfig,
  ]);

  const orbClassName = useMemo(() => {
    if (voiceBlocked) {
      return 'border-white/12 bg-slate-900/82 shadow-[0_18px_48px_rgba(15,23,42,0.34)]';
    }
    if (status === 'listening') {
      return 'border-rose-200/40 bg-[radial-gradient(circle_at_35%_30%,rgba(251,113,133,0.42),rgba(35,11,25,0.88)_72%)] shadow-[0_0_40px_rgba(251,113,133,0.35)]';
    }
    if (status === 'transcribing') {
      return 'border-amber-200/40 bg-[radial-gradient(circle_at_35%_30%,rgba(251,191,36,0.38),rgba(39,27,8,0.9)_72%)] shadow-[0_0_40px_rgba(251,191,36,0.32)]';
    }
    if (status === 'ready') {
      return 'border-emerald-200/40 bg-[radial-gradient(circle_at_35%_30%,rgba(52,211,153,0.36),rgba(7,29,24,0.9)_72%)] shadow-[0_0_40px_rgba(16,185,129,0.3)]';
    }
    if (status === 'error') {
      return 'border-rose-200/40 bg-[radial-gradient(circle_at_35%_30%,rgba(244,63,94,0.38),rgba(34,8,18,0.92)_72%)] shadow-[0_0_42px_rgba(244,63,94,0.3)]';
    }
    return 'border-sky-200/30 bg-[radial-gradient(circle_at_35%_30%,rgba(74,131,221,0.34),rgba(10,16,31,0.92)_72%)] shadow-[0_0_42px_rgba(74,131,221,0.28)]';
  }, [status, voiceBlocked]);

  return (
    <div className="flex h-screen w-screen items-end justify-start bg-transparent px-3 py-4">
      <div className="relative flex select-none flex-col items-start gap-3">
        <div className="pointer-events-none absolute bottom-0 left-0 h-[68px] w-[68px] rounded-full bg-sky-400/10 blur-2xl" />

        <div className="ml-1 flex max-w-[190px] flex-col items-start gap-2">
          {bubbles.map((bubble, index) => (
            <div
              key={bubble.id}
              className="animate-[overlay-bubble-in_220ms_ease-out] transition-transform duration-200"
              style={{ marginLeft: `${index * 8}px`, animationDelay: `${index * 40}ms` }}>
              <OverlayBubbleChip bubble={bubble} />
            </div>
          ))}
        </div>

        <div className="relative">
          {status === 'listening' ? (
            <>
              <span className="pointer-events-none absolute -inset-2 rounded-full border border-rose-300/30 animate-ping" />
              <span className="pointer-events-none absolute -inset-4 rounded-full border border-rose-200/15" />
            </>
          ) : null}

          <button
            type="button"
            aria-label={status === 'listening' ? 'Stop listening' : 'Start listening'}
            disabled={orbDisabled}
            onClick={handleMainButton}
            onContextMenu={event => {
              event.preventDefault();
              setVoiceCaptureEnabled(previous => !previous);
            }}
            onDoubleClick={() => {
              void appWindow.hide();
            }}
            className={`group relative flex h-[50px] w-[50px] items-center justify-center overflow-hidden rounded-full border transition-all duration-200 disabled:cursor-not-allowed disabled:opacity-60 ${orbClassName}`}
            title="Click to talk. Right-click to toggle voice. Double-click to hide.">
            <span className="pointer-events-none absolute inset-[5px] rounded-full border border-white/12" />
            <span className="pointer-events-none absolute inset-[11px] rounded-full bg-white/6 blur-[1px]" />
            <div className="pointer-events-none h-[24px] w-[24px] opacity-95 transition-transform duration-300 group-hover:scale-105">
              <RotatingTetrahedronCanvas />
            </div>
          </button>

          <button
            type="button"
            onMouseDown={event => {
              event.preventDefault();
              void appWindow.startDragging();
            }}
            className="absolute -right-3 -top-2 flex h-5 min-w-5 items-center justify-center rounded-full border border-white/12 bg-slate-950/78 px-1.5 text-[9px] font-medium uppercase tracking-[0.16em] text-white/80 shadow-[0_10px_24px_rgba(2,6,23,0.34)] backdrop-blur-md transition hover:bg-slate-900/88"
            title="Drag overlay">
            drag
          </button>
        </div>
      </div>
    </div>
  );
}
