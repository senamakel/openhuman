import {
  Fit,
  Layout,
  useRive,
  useViewModel,
  useViewModelInstance,
  useViewModelInstanceColor,
  useViewModelInstanceEnum,
} from '@rive-app/react-webgl2';
import debug from 'debug';
import { type FC, useEffect, useRef, useState } from 'react';

import type { MascotFace } from './Ghosty';
import { loadManifestRiv } from './manifest/manifestService';
import { pickIdleFlourish, resolveFaceToPose, resolveVisemeCode } from './manifest/stateEngine';
import type {
  MascotManifestChannel,
  MascotManifestEntry,
  MascotStateEngine,
} from './manifest/types';
import { MASCOT_STATE_MACHINE } from './riveMaps';
import { RiveMascot } from './RiveMascot';

const log = debug('human:mascot:manifest-rive');

/** Idle dwell before the mascot drifts into a flourish (ms), randomised. */
const AMBIENT_IDLE_MIN_MS = 6_000;
const AMBIENT_IDLE_MAX_MS = 12_000;
/** How long a flourish is held before returning to the resting pose (ms). */
const AMBIENT_HOLD_MIN_MS = 2_500;
const AMBIENT_HOLD_MAX_MS = 5_000;
/** Fallback channel auto-cycle interval when the manifest omits one (ms). */
const CHANNEL_CYCLE_FALLBACK_MS = 2_500;

function randBetween(min: number, max: number): number {
  return min + Math.random() * (max - min);
}

const RIVE_LAYOUT = new Layout({ fit: Fit.Contain });

export interface ManifestRiveMascotProps {
  /** The manifest entry to render. Its runtime `.riv` is loaded + cached. */
  entry: MascotManifestEntry;
  face?: MascotFace;
  size?: number | string;
  primaryColor?: number;
  secondaryColor?: number;
  /** Raw Oculus 15-set viseme code; normalised to this mascot's vocabulary. */
  visemeCode?: string;
  /** Drift through this mascot's idle pose cycle + auto-cycle its channels. */
  idlePoseRotation?: boolean;
}

/**
 * Render a manifest mascot from its loaded `.riv` buffer. Split out from the
 * loader so every Rive hook runs against a present buffer (calling the Rive
 * hooks with no source then swapping in a buffer mid-mount destabilises the
 * runtime). The parent keys this by mascot id so a new selection remounts it.
 */
const ManifestRiveStage: FC<{
  buffer: ArrayBuffer;
  engine: MascotStateEngine;
  channels: MascotManifestChannel[];
  face: MascotFace;
  size: number | string;
  primaryColor?: number;
  secondaryColor?: number;
  visemeCode: string;
  idlePoseRotation: boolean;
}> = ({
  buffer,
  engine,
  channels,
  face,
  size,
  primaryColor,
  secondaryColor,
  visemeCode,
  idlePoseRotation,
}) => {
  const { rive, RiveComponent } = useRive({
    buffer,
    stateMachines: MASCOT_STATE_MACHINE,
    autoplay: true,
    layout: RIVE_LAYOUT,
  });

  const viewModel = useViewModel(rive, { useDefault: true });
  const vmInstance = useViewModelInstance(viewModel, { useDefault: true, rive });
  const { setValue: setPose } = useViewModelInstanceEnum('pose', vmInstance);
  const { setValue: setMouthVisemeCode } = useViewModelInstanceEnum('mouthVisemeCode', vmInstance);
  const { setValue: setPrimaryColor } = useViewModelInstanceColor('primaryColor', vmInstance);
  const { setValue: setSecondaryColor } = useViewModelInstanceColor('secondaryColor', vmInstance);

  const basePose = resolveFaceToPose(face, engine);
  const restPose = engine.states.idle;

  // Driven (face-derived) pose. A real activity pose always wins; the resting
  // pose is what the idle scheduler below is free to override.
  useEffect(() => {
    setPose(basePose);
  }, [basePose, setPose]);

  // Idle pose rotation, scoped to this mascot's idlePoseCycle. Same self-
  // rescheduling shape as RiveMascot; only runs while enabled AND resting.
  const setPoseRef = useRef(setPose);
  setPoseRef.current = setPose;
  useEffect(() => {
    if (!idlePoseRotation || basePose !== restPose) return;
    let timer: number | undefined;
    let current = restPose;
    const toRest = () => {
      current = restPose;
      setPoseRef.current(restPose);
      timer = window.setTimeout(toFlourish, randBetween(AMBIENT_IDLE_MIN_MS, AMBIENT_IDLE_MAX_MS));
    };
    const toFlourish = () => {
      current = pickIdleFlourish(engine, current === restPose ? undefined : current);
      log('idle flourish → %s', current);
      setPoseRef.current(current);
      timer = window.setTimeout(toRest, randBetween(AMBIENT_HOLD_MIN_MS, AMBIENT_HOLD_MAX_MS));
    };
    timer = window.setTimeout(toFlourish, randBetween(AMBIENT_IDLE_MIN_MS, AMBIENT_IDLE_MAX_MS));
    return () => {
      if (timer !== undefined) window.clearTimeout(timer);
      setPoseRef.current(restPose);
    };
  }, [idlePoseRotation, basePose, restPose, engine]);

  useEffect(() => {
    setMouthVisemeCode(resolveVisemeCode(visemeCode, engine));
  }, [visemeCode, engine, setMouthVisemeCode]);

  useEffect(() => {
    if (primaryColor !== undefined) setPrimaryColor(primaryColor);
  }, [primaryColor, setPrimaryColor]);

  useEffect(() => {
    if (secondaryColor !== undefined) setSecondaryColor(secondaryColor);
  }, [secondaryColor, setSecondaryColor]);

  return (
    <div
      style={{
        width: typeof size === 'number' ? `${size}px` : size,
        height: typeof size === 'number' ? `${size}px` : size,
      }}
      data-face={face}>
      <RiveComponent />
      {channels.map(channel => (
        <ChannelDriver
          key={channel.key}
          channel={channel}
          vmInstance={vmInstance}
          autoCycle={idlePoseRotation}
        />
      ))}
    </div>
  );
};

/**
 * Drives one optional enum channel (e.g. `eyes`) onto the view model. Each
 * channel is its own component so the rules-of-hooks count stays stable, and
 * auto-cycles its value on a timer when the manifest marks it cyclable and the
 * mascot is in its "feel alive" mode.
 */
const ChannelDriver: FC<{
  channel: MascotManifestChannel;
  vmInstance: ReturnType<typeof useViewModelInstance>;
  autoCycle: boolean;
}> = ({ channel, vmInstance, autoCycle }) => {
  const { setValue } = useViewModelInstanceEnum(channel.key, vmInstance);
  const [value, setVal] = useState<string>(channel.default ?? channel.values[0]);

  useEffect(() => {
    if (value != null) setValue(value);
  }, [value, setValue]);

  useEffect(() => {
    if (!autoCycle || !channel.cycle?.enabled || channel.values.length < 2) return;
    const interval = channel.cycle.intervalMs ?? CHANNEL_CYCLE_FALLBACK_MS;
    const sequential = channel.cycle.order === 'sequential';
    let index = 0;
    const timer = window.setInterval(() => {
      if (sequential) {
        index = (index + 1) % channel.values.length;
        setVal(channel.values[index]);
      } else {
        setVal(channel.values[Math.floor(Math.random() * channel.values.length)]);
      }
    }, interval);
    return () => window.clearInterval(timer);
  }, [autoCycle, channel]);

  return null;
};

/**
 * Load a manifest mascot's `.riv` and render it. While the buffer resolves —
 * or if it fails — the bundled default mascot keeps the stage alive and still
 * lip-syncs, so a slow GitHub fetch never blanks the Human page.
 */
export const ManifestRiveMascot: FC<ManifestRiveMascotProps> = ({
  entry,
  face = 'idle',
  size = '100%',
  primaryColor,
  secondaryColor,
  visemeCode = 'sil',
  idlePoseRotation = false,
}) => {
  const [buffer, setBuffer] = useState<ArrayBuffer | null>(null);
  const [failed, setFailed] = useState(false);

  // Callers key this component by `entry.id`, so a new selection remounts it
  // with fresh state — the effect only ever resolves the buffer (or marks
  // failure) for the entry it mounted with.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const buf = await loadManifestRiv(entry);
        if (!cancelled) setBuffer(buf);
      } catch (err) {
        if (!cancelled) {
          log('failed to load mascot %s: %o', entry.id, err);
          setFailed(true);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [entry]);

  const fallbackProps = { face, size, primaryColor, secondaryColor, visemeCode, idlePoseRotation };
  if (failed || !buffer) return <RiveMascot key="default" {...fallbackProps} />;

  return (
    <ManifestRiveStage
      key={`buf-${entry.id}`}
      buffer={buffer}
      engine={entry.stateEngine}
      channels={entry.stateEngine.channels ?? []}
      face={face}
      size={size}
      primaryColor={primaryColor}
      secondaryColor={secondaryColor}
      visemeCode={visemeCode}
      idlePoseRotation={idlePoseRotation}
    />
  );
};
