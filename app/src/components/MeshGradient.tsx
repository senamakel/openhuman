import { useEffect, useRef, useState } from 'react';

import { Gradient } from '../lib/meshGradient';

/**
 * Animated WebGL mesh gradient background (Stripe-style).
 * Renders behind the dotted-canvas overlay so dots remain visible on top.
 * Catches WebGL errors gracefully so the app still works when the GPU context
 * is unavailable or lost (e.g. Tauri WebView on some platforms).
 */
export default function MeshGradient() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [isDark, setIsDark] = useState(() =>
    typeof document !== 'undefined' && document.documentElement.classList.contains('dark')
  );

  useEffect(() => {
    if (typeof document === 'undefined') return;
    const obs = new MutationObserver(() => {
      setIsDark(document.documentElement.classList.contains('dark'));
    });
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });
    return () => obs.disconnect();
  }, []);

  useEffect(() => {
    let gradient: InstanceType<typeof Gradient> | null = null;

    try {
      gradient = new Gradient();
      gradient.initGradient('#mesh-gradient');
    } catch (err) {
      console.warn('[MeshGradient] WebGL init failed, gradient disabled:', err);
      gradient = null;
    }

    return () => {
      try {
        if (gradient) {
          gradient.disconnect();
          gradient.pause();
        }
      } catch {
        // Cleanup is best-effort.
      }
    };
  }, []);

  return (
    <canvas
      ref={canvasRef}
      id="mesh-gradient"
      data-transition-in
      className={`absolute inset-0 w-full h-full ${isDark ? 'opacity-100' : 'opacity-30'}`}
      style={
        (isDark
          ? {
              '--gradient-color-1': '#0a0a0a',
              '--gradient-color-2': '#1e3a8a', // primary-900 deep ocean
              '--gradient-color-3': '#1d4ed8', // primary-700
              '--gradient-color-4': '#172554', // primary-950
            }
          : {
              // Cohesive ocean → lavender → mint sweep, no white. Saturated
              // enough that the bg dotted-canvas reads as a real surface
              // (not a washed-out gradient) while staying premium-soft.
              '--gradient-color-1': '#93C5FD', // primary-300
              '--gradient-color-2': '#9B8AFB', // accent.lavender
              '--gradient-color-3': '#7DD3FC', // accent.sky
              '--gradient-color-4': '#BFDBFE', // primary-200
            }) as React.CSSProperties
      }
    />
  );
}
