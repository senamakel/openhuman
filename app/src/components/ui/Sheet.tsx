import { cva, type VariantProps } from 'class-variance-authority';
import { Dialog as DialogPrimitive } from 'radix-ui';
import { type ComponentPropsWithoutRef, type ReactNode } from 'react';

import { cn } from '../../lib/cn';
import { DialogOverlay } from './Dialog';

export const SheetRoot = DialogPrimitive.Root;
export const SheetTrigger = DialogPrimitive.Trigger;
export const SheetClose = DialogPrimitive.Close;
export const SheetTitle = DialogPrimitive.Title;
export const SheetDescription = DialogPrimitive.Description;

/**
 * A side-anchored panel. Radix has no Drawer primitive, so this is a Dialog
 * pinned to an edge — which is what the six hand-rolled drawers in the app
 * (`SubagentDrawer`, `MeetDefaultsDrawer`, `FlowRunInspectorDrawer`,
 * `NodeConfigDrawer`, `FlowRunsDrawer`, `WhatLeavesMyComputerSheet`) each
 * reimplemented, none of them with a focus trap.
 *
 * Deliberately not `vaul`: its value is drag-to-dismiss on touch, and this is a
 * desktop app with essentially no touch surface.
 */
export const sheetVariants = cva(
  'fixed z-50 flex flex-col bg-surface shadow-large focus:outline-hidden',
  {
    variants: {
      side: {
        right: 'inset-y-0 right-0 h-full w-full max-w-md border-l border-line',
        left: 'inset-y-0 left-0 h-full w-full max-w-md border-r border-line',
        top: 'inset-x-0 top-0 w-full border-b border-line',
        bottom: 'inset-x-0 bottom-0 w-full border-t border-line',
      },
    },
    defaultVariants: { side: 'right' },
  }
);

export interface SheetContentProps
  extends
    ComponentPropsWithoutRef<typeof DialogPrimitive.Content>,
    VariantProps<typeof sheetVariants> {
  /** Portal target — see `DialogContent`. Defaults to `document.body`. */
  container?: HTMLElement | null;
  overlayClassName?: string;
  children: ReactNode;
}

export const SheetContent = ({
  side,
  className,
  overlayClassName,
  container,
  children,
  ...rest
}: SheetContentProps) => (
  <DialogPrimitive.Portal container={container ?? undefined}>
    <DialogOverlay className={overlayClassName} />
    <DialogPrimitive.Content
      data-slot="sheet-content"
      data-side={side ?? 'right'}
      className={cn(sheetVariants({ side }), className)}
      {...rest}>
      {children}
    </DialogPrimitive.Content>
  </DialogPrimitive.Portal>
);

export default SheetRoot;
