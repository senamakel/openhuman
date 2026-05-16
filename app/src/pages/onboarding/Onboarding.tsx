import { Navigate, Route, Routes } from 'react-router-dom';

import OnboardingLayout from './OnboardingLayout';
import ApiKeysPage from './pages/ApiKeysPage';
import RuntimeChoicePage from './pages/RuntimeChoicePage';
import WelcomePage from './pages/WelcomePage';

/**
 * Routed onboarding flow.
 *
 *   welcome → runtime-choice → (api-keys if Custom) → /home
 *
 * Gmail / Composio (`/onboarding/skills`) and the Composio-driven context
 * gathering (`/onboarding/context`) used to live here but were removed from
 * the default flow: requiring a Gmail connect on first launch scared users
 * off, and the context step depended on it. The step + page files are kept
 * on disk so we can reintroduce them as optional steps later.
 */
const Onboarding = () => {
  return (
    <Routes>
      <Route element={<OnboardingLayout />}>
        <Route index element={<Navigate to="welcome" replace />} />
        <Route path="welcome" element={<WelcomePage />} />
        <Route path="runtime-choice" element={<RuntimeChoicePage />} />
        <Route path="api-keys" element={<ApiKeysPage />} />
        <Route path="*" element={<Navigate to="welcome" replace />} />
      </Route>
    </Routes>
  );
};

export default Onboarding;
