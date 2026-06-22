import { useT } from '../../lib/i18n/I18nContext';
import type { WalkthroughStepState } from '../../pages/onboarding/OnboardingContext';
import { useWalkthroughUI } from './WalkthroughProvider';

interface WalkthroughActionCardProps {
  step: WalkthroughStepState;
}

/**
 * An action card in the walkthrough flow. Displays the step name, description,
 * and a completion toggle. When completed, shows a checkmark with a subtle
 * animation — matching the "completed action cards" design reference.
 */
const WalkthroughActionCard = ({ step }: WalkthroughActionCardProps) => {
  const { completeStep, stepLabels, stepDescriptions } = useWalkthroughUI();
  const { t } = useT();

  const label = stepLabels[step.key] ?? step.key;
  const description = stepDescriptions[step.key] ?? '';

  return (
    <button
      type="button"
      onClick={() => completeStep(step.key)}
      disabled={step.completed}
      className={`
        w-full text-left p-4 rounded-xl border transition-all duration-200
        flex items-start gap-3 group
        ${
          step.completed
            ? 'bg-[#2F6EF4]/5 border-[#2F6EF4]/30 cursor-default'
            : 'bg-white dark:bg-neutral-900 border-stone-200 dark:border-neutral-800 hover:border-[#2F6EF4]/40 hover:shadow-md hover:shadow-[#2F6EF4]/5 active:scale-[0.98]'
        }
      `}
      aria-label={
        step.completed
          ? t('walkthrough.card.completedAria', `${label} — completed`).replace('{label}', label)
          : t('walkthrough.card.actionAria', `Complete ${label}`).replace('{label}', label)
      }>
      {/* Status indicator */}
      <div
        className={`
          shrink-0 w-6 h-6 rounded-full border-2 flex items-center justify-center
          transition-all duration-300 mt-0.5
          ${
            step.completed
              ? 'bg-[#2F6EF4] border-[#2F6EF4]'
              : 'border-stone-300 dark:border-neutral-600 group-hover:border-[#2F6EF4]/60'
          }
        `}>
        {step.completed && (
          <svg
            className="w-3.5 h-3.5 text-white animate-in zoom-in duration-200"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="3"
            strokeLinecap="round"
            strokeLinejoin="round">
            <polyline points="20 6 9 17 4 12" />
          </svg>
        )}
      </div>

      {/* Content */}
      <div className="flex-1 min-w-0">
        <h4
          className={`
            text-sm font-semibold transition-colors
            ${
              step.completed
                ? 'text-[#2F6EF4]'
                : 'text-stone-900 dark:text-neutral-100 group-hover:text-[#2F6EF4]'
            }
          `}>
          {label}
        </h4>
        {description && (
          <p className="text-xs text-stone-500 dark:text-neutral-400 mt-0.5 leading-relaxed">
            {description}
          </p>
        )}
      </div>

      {/* Arrow indicator for incomplete steps */}
      {!step.completed && (
        <svg
          className="w-4 h-4 text-stone-300 dark:text-neutral-600 group-hover:text-[#2F6EF4] shrink-0 mt-1 transition-colors"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round">
          <polyline points="9 18 15 12 9 6" />
        </svg>
      )}
    </button>
  );
};

export default WalkthroughActionCard;
