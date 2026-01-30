/**
 * Scheduling utilities
 */

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyFunction = (...args: any[]) => any;

export function throttle<F extends AnyFunction>(
  fn: F,
  ms: number,
  shouldRunFirst = true,
): F {
  let waiting = false;
  let pendingArgs: Parameters<F> | null = null;

  return ((...args: Parameters<F>) => {
    if (waiting) {
      pendingArgs = args;
      return;
    }

    if (shouldRunFirst) {
      fn(...args);
    } else {
      pendingArgs = args;
    }

    waiting = true;
    setTimeout(() => {
      waiting = false;
      if (pendingArgs) {
        fn(...pendingArgs);
        pendingArgs = null;
      }
    }, ms);
  }) as unknown as F;
}

export function debounce<F extends AnyFunction>(
  fn: F,
  ms: number,
  shouldRunFirst = false,
  shouldRunLast = true,
): F {
  let waitingTimeout: ReturnType<typeof setTimeout> | null = null;

  return ((...args: Parameters<F>) => {
    if (waitingTimeout) {
      clearTimeout(waitingTimeout);
    } else if (shouldRunFirst) {
      fn(...args);
    }

    waitingTimeout = setTimeout(() => {
      if (shouldRunLast) {
        fn(...args);
      }
      waitingTimeout = null;
    }, ms);
  }) as unknown as F;
}

export const pause = (ms: number): Promise<void> =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));
