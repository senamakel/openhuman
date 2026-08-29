import { cva, type VariantProps } from 'class-variance-authority';
import { type ComponentPropsWithRef } from 'react';

import { cn } from '../../lib/cn';

/**
 * A static status surface — no Radix primitive, because there is no
 * interaction to manage: an alert is read, not operated.
 *
 * ROLE. `role="alert"` is applied to `destructive` and `warning` only. It maps
 * to an assertive live region, which interrupts whatever a screen reader is
 * saying; using it for an `info` panel that is simply present on page load
 * makes every visit talk over itself. Informational variants stay a plain
 * container and are read in document order. A caller that renders a
 * *dynamically arriving* info alert can still pass `role`/`aria-live`
 * explicitly — `...rest` wins over the default.
 */
export const alertVariants = cva('relative flex w-full gap-3 rounded-xl border px-4 py-3 text-sm', {
  variants: {
    variant: {
      default: 'border-line bg-surface text-content',
      info: 'border-primary-200 bg-primary-50 text-primary-700 dark:border-primary-500/30 dark:bg-primary-500/10 dark:text-primary-200',
      success:
        'border-sage-200 bg-sage-50 text-sage-700 dark:border-sage-500/30 dark:bg-sage-500/10 dark:text-sage-200',
      warning:
        'border-amber-200 bg-amber-50 text-amber-700 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-200',
      destructive:
        'border-coral-200 bg-coral-50 text-coral-600 dark:border-coral-500/30 dark:bg-coral-500/10 dark:text-coral-200',
    },
  },
  defaultVariants: { variant: 'default' },
});

export type AlertVariant = NonNullable<VariantProps<typeof alertVariants>['variant']>;

/** The variants urgent enough to justify an assertive live region. */
const ASSERTIVE_VARIANTS: readonly AlertVariant[] = ['destructive', 'warning'];

export interface AlertProps
  extends ComponentPropsWithRef<'div'>, VariantProps<typeof alertVariants> {}

export const Alert = ({ className, variant, ...rest }: AlertProps) => {
  const resolved: AlertVariant = variant ?? 'default';
  return (
    <div
      data-slot="alert"
      data-variant={resolved}
      role={ASSERTIVE_VARIANTS.includes(resolved) ? 'alert' : undefined}
      className={cn(alertVariants({ variant }), className)}
      {...rest}
    />
  );
};

export const AlertTitle = ({ className, ...rest }: ComponentPropsWithRef<'div'>) => (
  <div
    data-slot="alert-title"
    className={cn('font-medium leading-snug tracking-tight', className)}
    {...rest}
  />
);

export const AlertDescription = ({ className, ...rest }: ComponentPropsWithRef<'div'>) => (
  <div
    data-slot="alert-description"
    className={cn('text-sm leading-relaxed opacity-90', className)}
    {...rest}
  />
);

export default Alert;
