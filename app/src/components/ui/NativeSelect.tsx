import { forwardRef, type SelectHTMLAttributes } from 'react';

import { cn } from '../../lib/cn';

export type NativeSelectSize = 'sm' | 'md';

export interface NativeSelectProps extends SelectHTMLAttributes<HTMLSelectElement> {
  inputSize?: NativeSelectSize;
  'data-testid'?: string;
}

/**
 * A styled native `<select>`, kept deliberately alongside Radix `Select`.
 *
 * Radix `Select` is worth it for short lists that need custom item rendering.
 * It is the wrong tool for model pickers and timezone lists — it virtualizes
 * nothing, so a few hundred options are slow, and it is the hardest primitive
 * to drive under jsdom. The native control also remains the right answer on the
 * mobile route tree, where the OS picker beats any HTML popup.
 */
const CHEVRON_BG =
  "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath d='M2 4l4 4 4-4' stroke='%23a3a3a3' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round' fill='none'/%3E%3C/svg%3E\")";

const NativeSelect = forwardRef<HTMLSelectElement, NativeSelectProps>(
  ({ inputSize = 'md', className, 'data-testid': testId, style, ...rest }, ref) => (
    <select
      ref={ref}
      data-slot="native-select"
      data-size={inputSize}
      data-testid={testId}
      className={cn(
        'block cursor-pointer appearance-none rounded-lg border border-line-strong bg-surface bg-no-repeat pr-7 text-sm text-content',
        'transition-colors duration-150',
        'focus:border-primary-500 focus:outline-hidden focus:ring-2 focus:ring-primary-500/20',
        'disabled:cursor-not-allowed disabled:opacity-50',
        inputSize === 'sm' ? 'h-8 pl-2.5' : 'h-9 pl-3',
        className
      )}
      style={{
        backgroundImage: CHEVRON_BG,
        backgroundPosition: 'right 0.5rem center',
        backgroundSize: '12px 12px',
        ...style,
      }}
      {...rest}
    />
  )
);
NativeSelect.displayName = 'NativeSelect';

export default NativeSelect;
