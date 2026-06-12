import debug from 'debug';
import { Outlet, useLocation } from 'react-router-dom';

import { SettingsLayoutProvider } from './SettingsLayoutContext';
import SettingsSidebar from './SettingsSidebar';
import SettingsSubNav from './SettingsSubNav';

const log = debug('settings:layout');

/**
 * Two-pane settings shell.
 *
 * Wide (md+): persistent grouped sidebar on the left, the routed panel on the
 * right. Narrow (and the iOS client): the sidebar IS the /settings index page
 * and panels render full-width with a back button — the classic drill-down.
 *
 * Scrolling is provided by the AppShell page scroller; the sidebar sticks to
 * the top of it and scrolls independently when taller than the viewport.
 */
const SettingsLayout = () => {
  const location = useLocation();
  // /settings (no sub-route) → narrow shows the sidebar as the page.
  const isIndex = location.pathname.replace(/\/+$/, '') === '/settings';
  log('render pathname=%s isIndex=%s', location.pathname, isIndex);

  return (
    <SettingsLayoutProvider value={{ inTwoPaneShell: true }}>
      <div className="flex items-start">
        {/* Sidebar — full-width page on narrow index, persistent column on md+ */}
        <aside
          className={`w-full md:w-64 lg:w-72 md:shrink-0 md:sticky md:top-0 md:max-h-[calc(100vh-5rem)] md:overflow-y-auto p-2 md:border-r md:border-stone-200 md:dark:border-neutral-800 ${
            isIndex ? 'block' : 'hidden md:block'
          }`}>
          <SettingsSidebar />
        </aside>

        {/* Content pane */}
        <main className={`flex-1 min-w-0 ${isIndex ? 'hidden md:block' : 'block'}`}>
          <div className="px-4 pt-4 -mb-4">
            <div className="max-w-2xl mx-auto">
              <SettingsSubNav />
            </div>
          </div>
          <Outlet />
        </main>
      </div>
    </SettingsLayoutProvider>
  );
};

export default SettingsLayout;
