import { HashRouter as Router } from "react-router-dom";
import { Provider } from "react-redux";
import { PersistGate } from "redux-persist/integration/react";
import * as Sentry from "@sentry/react";
import { store, persistor } from "./store";
import UserProvider from "./providers/UserProvider";
import SocketProvider from "./providers/SocketProvider";
import TelegramProvider from "./providers/TelegramProvider";
import AIProvider from "./providers/AIProvider";
import AppRoutes from "./AppRoutes";

function App() {
  return (
    <Sentry.ErrorBoundary fallback={<div>Something went wrong.</div>}>
      <Provider store={store}>
        <PersistGate loading={null} persistor={persistor}>
          <UserProvider>
            <SocketProvider>
              <TelegramProvider>
                <AIProvider>
                <Router>
                  <div className="relative min-h-screen">
                    <div className="pointer-events-none fixed inset-x-0 bottom-3 flex justify-center z-50">
                      <div className="bg-black/30 px-3 py-1 text-[11px] uppercase tracking-[0.18em] text-white/40">
                        AlphaHuman is in alpha &mdash; share feedback by messaging
                        the Telegram bot.
                      </div>
                    </div>
                    <AppRoutes />
                  </div>
                </Router>
                </AIProvider>
              </TelegramProvider>
            </SocketProvider>
          </UserProvider>
        </PersistGate>
      </Provider>
    </Sentry.ErrorBoundary>
  );
}

export default App;
