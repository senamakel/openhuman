// `pnpm tauri …` entry point: runs the Tauri CLI against the desktop shell in
// crates/openhuman-app while keeping the caller's cwd, so relative CLI
// arguments (`-c path.json`, `--target …`) still resolve from app/.
//
// A node wrapper rather than `cd ../crates/openhuman-app && …` because pnpm runs
// package scripts through cmd.exe on Windows, which cannot parse that chain.
// TAURI_APP_PATH is the CLI's own project-dir override; the cwd (app/, which has
// a package.json) stays the frontend dir that before-commands run in.
const path = require('node:path');

process.env.TAURI_APP_PATH ||= path.resolve(__dirname, '../../crates/openhuman-app');
require('@tauri-apps/cli/tauri.js');
