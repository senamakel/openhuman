import type { ReactNode } from 'react';

interface WelcomeHeroCardProps {
  icon: ReactNode;
  title: string;
  description: string;
}

const WelcomeHeroCard = ({ icon, title, description }: WelcomeHeroCardProps) => (
  <div className="flex flex-col items-center text-center gap-3 p-5 rounded-2xl bg-white dark:bg-neutral-800/50 border border-neutral-200/60 dark:border-neutral-700/40 shadow-soft hover:shadow-md transition-shadow">
    <div className="flex items-center justify-center w-11 h-11 rounded-xl bg-primary-50 dark:bg-primary-500/10 text-primary-600 dark:text-primary-400">
      {icon}
    </div>
    <h3 className="text-base font-semibold text-stone-900 dark:text-neutral-100">{title}</h3>
    <p className="text-xs text-stone-500 dark:text-neutral-400 leading-relaxed max-w-[220px]">
      {description}
    </p>
  </div>
);

export default WelcomeHeroCard;
