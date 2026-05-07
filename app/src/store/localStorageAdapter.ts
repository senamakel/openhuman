/**
 * Promise-based wrapper around `window.localStorage` that matches
 * redux-persist's storage contract (`getItem` / `setItem` / `removeItem`,
 * each returning a Promise).
 *
 * Inlined here rather than imported from `redux-persist/lib/storage` because
 * Vite's CJS dep pre-bundling can resolve that default export to the module
 * namespace under some configurations, leaving `storage.getItem` undefined
 * and crashing rehydrate on cold boot. Owning the adapter sidesteps the
 * interop hazard entirely.
 *
 * All three operations swallow synchronous `localStorage` failures (quota
 * exceeded, private-browsing mode where access throws, etc.) so persist
 * code never sees a rejected promise and proceeds with the in-memory store.
 */
export const localStorageAdapter = {
  getItem: (key: string): Promise<string | null> =>
    Promise.resolve(
      (() => {
        try {
          return localStorage.getItem(key);
        } catch {
          return null;
        }
      })()
    ),
  setItem: (key: string, value: string): Promise<void> =>
    Promise.resolve(
      (() => {
        try {
          localStorage.setItem(key, value);
        } catch {
          /* ignore quota / unavailable */
        }
      })()
    ),
  removeItem: (key: string): Promise<void> =>
    Promise.resolve(
      (() => {
        try {
          localStorage.removeItem(key);
        } catch {
          /* ignore */
        }
      })()
    ),
};
