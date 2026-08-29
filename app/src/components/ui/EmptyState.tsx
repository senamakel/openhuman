import { cn } from '../../lib/cn';

export interface EmptyStateProps {
  label: string;
  className?: string;
}

const EmptyState = ({ label, className }: EmptyStateProps) => (
  <p
    data-slot="empty-state"
    className={cn('px-4 py-4 text-xs italic text-content-faint', className)}>
    {label}
  </p>
);

export default EmptyState;
