import debug from 'debug';
import { Outlet } from 'react-router-dom';

import TwoPanelLayout from '../../layout/TwoPanelLayout';
import { SettingsLayoutProvider } from './SettingsLayoutContext';
import SettingsSidebar from './SettingsSidebar';
import SettingsSubNav from './SettingsSubNav';

const log = debug('settings:layout');

/**
 * Two-pane settings shell, built on the reusable {@link TwoPanelLayout}.
 *
 * The grouped navigation sidebar is always shown and the layout spans the full
 * width of the page; the sidebar is resizable (drag the divider) and its width
 * persists per user via the `layout` slice (id `settings`). Each pane scrolls
 * independently, so the nav and the routed panel never fight over one
 * scrollbar.
 */
const SettingsLayout = () => {
  log('render');

  return (
    <SettingsLayoutProvider value={{ inTwoPaneShell: true }}>
      <TwoPanelLayout
        id="settings"
        // Full width with the same card panes (bg / border / rounding) the
        // conversations two-pane uses — supplied by TwoPanelLayout's default
        // paneClassName, so we don't restyle here.
        className="h-full w-full p-4 pt-6"
        defaultSidebarVisible
        defaultSidebarWidth={288}
        minSidebarWidth={220}
        maxSidebarWidth={420}
        sidebar={
          <div className="h-full overflow-y-auto p-2">
            <SettingsSidebar />
          </div>
        }>
        <div className="h-full overflow-y-auto">
          <div className="px-4 pt-4 -mb-4">
            <div className="max-w-2xl mx-auto">
              <SettingsSubNav />
            </div>
          </div>
          <Outlet />
        </div>
      </TwoPanelLayout>
    </SettingsLayoutProvider>
  );
};

export default SettingsLayout;
