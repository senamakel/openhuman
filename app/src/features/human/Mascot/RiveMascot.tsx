import {
  Fit,
  Layout,
  useRive,
  useViewModel,
  useViewModelInstance,
  useViewModelInstanceBoolean,
} from '@rive-app/react-webgl2';
import { type FC, useEffect } from 'react';

import type { MascotFace } from './Ghosty';

export interface RiveMascotProps {
  face?: MascotFace;
  size?: number | string;
}

const SPEAKING_FACES: ReadonlySet<MascotFace> = new Set(['speaking', 'happy']);

const RIVE_LAYOUT = new Layout({ fit: Fit.Contain });

export const RiveMascot: FC<RiveMascotProps> = ({ face = 'idle', size = '100%' }) => {
  const { rive, RiveComponent } = useRive({
    src: '/tiny_mascot.riv',
    stateMachines: 'State Machine 1',
    autoplay: true,
    layout: RIVE_LAYOUT,
  });

  const viewModel = useViewModel(rive, { useDefault: true });
  const vmInstance = useViewModelInstance(viewModel, { useDefault: true, rive });
  const { setValue: setMouthOpen } = useViewModelInstanceBoolean('mouthOpen', vmInstance);

  useEffect(() => {
    setMouthOpen(SPEAKING_FACES.has(face!));
  }, [face, setMouthOpen]);

  return (
    <div
      style={{
        width: typeof size === 'number' ? `${size}px` : size,
        height: typeof size === 'number' ? `${size}px` : size,
      }}
      data-face={face}>
      <RiveComponent />
    </div>
  );
};
