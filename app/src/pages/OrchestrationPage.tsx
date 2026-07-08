/**
 * OrchestrationPage — the TinyPlace multi-agent orchestration surface, promoted
 * from a Brain sub-tab (`/brain?tab=tinyplace-orchestration`) into a first-class
 * sidebar destination at `/orchestration` (sits right after Workflows).
 *
 * Thin page wrapper: it owns only the standard scaffold + scroll container and
 * delegates all behavior to {@link TinyPlaceOrchestrationTab}, exactly as the
 * Brain page did before the promotion.
 */
import TinyPlaceOrchestrationTab from '../components/intelligence/TinyPlaceOrchestrationTab';
import PanelPage from '../components/layout/PanelPage';

export default function OrchestrationPage() {
  console.debug('[orchestration] page mount');
  return (
    <PanelPage contentClassName="p-4">
      <div className="mx-auto max-w-5xl animate-fade-up">
        <TinyPlaceOrchestrationTab />
      </div>
    </PanelPage>
  );
}
