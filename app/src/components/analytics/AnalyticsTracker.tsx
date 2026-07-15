import { useEffect } from 'react';
import { useLocation } from 'react-router-dom';

import {
  type AnalyticsEventName,
  type AnalyticsParams,
  trackEvent,
  trackPageView,
} from '../../services/analytics';

/** Standard route-view tracker. Mount once inside the active router. */
export function AnalyticsPageTracker() {
  const { pathname } = useLocation();
  useEffect(() => {
    trackPageView(pathname);
  }, [pathname]);
  return null;
}

/** Typed facade for successful non-click outcomes such as a sent message. */
export function trackAnalyticsEvent(event: AnalyticsEventName, properties?: AnalyticsParams): void {
  trackEvent(event, properties);
}
