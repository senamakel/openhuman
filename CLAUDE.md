# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Summary

Cross-platform crypto community communication platform built with **Tauri v2** (React 19 + Rust). Targets desktop (Windows, macOS) and mobile (Android, iOS). Features deep Telegram integration via MTProto, real-time Socket.io communication, and an MCP (Model Context Protocol) tool system for AI-driven Telegram interactions.

## Commands

```bash
# Frontend dev server only (port 1420)
npm run dev

# Desktop dev with hot-reload (starts Vite + Tauri)
npm run tauri dev

# Production build (TypeScript compile + Vite build + Tauri bundle)
npm run tauri build

# Debug build with .app bundle (required for deep link testing on macOS)
npm run tauri build -- --debug --bundles app

# Android
npm run tauri android dev
npm run tauri android build

# iOS
npm run tauri ios dev
npm run tauri ios build

# Rust checks
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml
```

No test framework is currently configured. No ESLint or Prettier configuration exists in the repo.

## Architecture

### Provider Chain (App.tsx)

The app wraps in this order: `Redux Provider` → `PersistGate` → `SocketProvider` → `TelegramProvider` → `BrowserRouter` → `AppRoutes`. This ordering matters because Socket.io and Telegram providers depend on Redux auth state.

### State Management (Redux Toolkit + Persist)

State lives in `src/store/` using Redux Toolkit slices:
- **authSlice** — JWT token, onboarding completion flag (persisted)
- **userSlice** — user profile
- **socketSlice** — connection status, socket ID
- **telegramSlice** — connection/auth status, chats, messages, threads (selectively persisted; loading/error states excluded)

Redux Persist stores auth and telegram state to localStorage. The telegram slice has a complex nested structure in `src/store/telegram/` with separate files for types, reducers, extraReducers, and thunks.

### Service Layer (Singletons)

- **mtprotoService** (`src/services/mtprotoService.ts`) — Telegram MTProto client via `telegram` npm package. Session stored in localStorage as `telegram_session`. Auto-retries FLOOD_WAIT up to 60s.
- **socketService** (`src/services/socketService.ts`) — Socket.io client. Auth token passed in socket `auth` object (not query string). Transports: polling first, then WebSocket.
- **apiClient** (`src/services/apiClient.ts`) — HTTP client for REST backend.

### MCP System (`src/lib/mcp/`)

Model Context Protocol implementation for AI tool execution over Socket.io:
- `transport.ts` — Socket.io JSON-RPC 2.0 transport with 30s timeout
- `telegram/server.ts` — TelegramMCPServer manages 99 tool definitions
- `telegram/tools/` — Individual tool files (one per Telegram API operation)
- Tools use `big-integer` library for Telegram's large integer IDs

### Routing (`src/AppRoutes.tsx`)

```
/           → Welcome (public)
/login      → Login (public)
/onboarding → Onboarding (protected, requires auth, not yet onboarded)
/home       → Home (protected, requires auth + onboarded)
*           → DefaultRedirect (routes based on auth state)
```

`PublicRoute` redirects authenticated users away. `ProtectedRoute` enforces auth and optionally onboarding status.

### Deep Link Auth Flow

Web-to-desktop handoff using `outsourced://` URL scheme:
1. User authenticates in browser
2. Browser redirects to `outsourced://auth?token=<loginToken>`
3. Tauri catches the deep link, Rust `exchange_token` command calls backend via `reqwest` (bypasses CORS)
4. Backend returns `sessionToken` + user object
5. App stores session in Redux, navigates to onboarding/home

Key file: `src/utils/desktopDeepLinkListener.ts` (lazy-loaded in `main.tsx`). Uses `localStorage.deepLinkHandled` flag to prevent infinite reload loops. Deep links do NOT work in `tauri dev` on macOS — must use built `.app` bundle.

### Rust Backend (`src-tauri/src/lib.rs`)

Minimal — two Tauri commands:
- `greet` — demo command
- `exchange_token` — CORS-free HTTP POST to backend for token exchange

Deep link plugin registered at setup. `register_all()` called only on Windows/Linux (panics on macOS).

## Environment Variables

Set in `.env` (Vite exposes `VITE_*` prefixed vars):

| Variable | Purpose |
|----------|---------|
| `VITE_BACKEND_URL` | Backend API URL (default: `http://localhost:5005`) |
| `VITE_TELEGRAM_API_ID` | Telegram MTProto API ID |
| `VITE_TELEGRAM_API_HASH` | Telegram MTProto API hash |
| `VITE_TELEGRAM_BOT_USERNAME` | Telegram bot username |
| `VITE_TELEGRAM_BOT_ID` | Telegram bot numeric ID |
| `VITE_DEBUG` | Debug mode flag |

Production defaults are in `src/utils/config.ts`.

## Key Patterns

- **Node polyfills**: Vite config (`vite.config.ts`) polyfills `buffer`, `process`, `util`, `os`, `crypto`, `stream` for the `telegram` npm package which requires Node APIs.
- **Telegram IDs**: Use `big-integer` library, not native JS numbers (Telegram IDs exceed `Number.MAX_SAFE_INTEGER`).
- **MCP tool files**: Each tool in `src/lib/mcp/telegram/tools/` exports a handler conforming to `TelegramMCPToolHandler` interface. Tool names are typed in `src/lib/mcp/telegram/types.ts`.
- **Tauri IPC**: Frontend calls Rust via `invoke()` from `@tauri-apps/api/core`. Rust commands are registered in `generate_handler![]` macro.
- **CORS workaround**: External HTTP requests from the WebView hit CORS. Use Rust `reqwest` via Tauri commands instead of browser `fetch()`.

## Platform Gotchas

- **macOS deep links**: Require `.app` bundle (not `tauri dev`). Clear WebKit caches when debugging stale content: `rm -rf ~/Library/WebKit/com.megamind.tauri-app ~/Library/Caches/com.megamind.tauri-app`
- **Cargo caching**: May serve stale frontend assets on incremental builds. Run `cargo clean --manifest-path src-tauri/Cargo.toml` if the app shows outdated UI.
- **`window.__TAURI__`**: Not available at module load time. Use dynamic `import()` and try/catch for Tauri plugin calls.
