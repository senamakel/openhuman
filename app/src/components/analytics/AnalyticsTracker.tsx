import {
  Children,
  cloneElement,
  type ReactElement,
  type MouseEvent as ReactMouseEvent,
  useEffect,
} from 'react';
import { useLocation } from 'react-router-dom';

import {
  type AnalyticsEventName,
  type AnalyticsParams,
  trackEvent,
  trackPageView,
} from '../../services/analytics';

interface TrackableChildProps {
  'data-analytics-id'?: string;
  onClick?: (event: ReactMouseEvent<HTMLElement>) => void;
}

export interface TrackedInteractionProps {
  /** Stable, content-free identifier used in analytics dashboards. */
  id: string;
  /** Exactly one clickable React element. */
  children: ReactElement<TrackableChildProps>;
  /** Optional semantic event in addition to the automatic `ui_click`. */
  event?: AnalyticsEventName;
  /** Privacy-safe event dimensions. Never pass user-authored content. */
  properties?: AnalyticsParams;
}

/**
 * Standard wrapper for clickable UI.
 *
 * It stamps the child with the identifier consumed by the app-wide delegated
 * interaction tracker, while preserving the child's existing click handler.
 * Feature code only needs an explicit `event` for a semantic click-attempt;
 * successful lifecycle outcomes should use {@link trackAnalyticsEvent} after
 * the operation succeeds.
 */
export function TrackedInteraction({ id, children, event, properties }: TrackedInteractionProps) {
  const child = Children.only(children);
  return cloneElement(child, {
    'data-analytics-id': id,
    onClick: (clickEvent: ReactMouseEvent<HTMLElement>) => {
      child.props.onClick?.(clickEvent);
      if (event && !clickEvent.defaultPrevented) {
        trackEvent(event, properties);
      }
    },
  });
}

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
