// Tauri config overrides applied at CI build time on top of the static
// `app/src-tauri/tauri.conf.json`. Anything returned here is merged via
// `tauri build --config <json>` and wins over the static file.
//
// The updater config (endpoint + minisign pubkey + createUpdaterArtifacts)
// USED to live here, sourced from `UPDATER_PUBLIC_KEY` / `UPDATER_ENDPOINT`
// env vars. That indirection caused a real outage class: if
// `UPDATER_PUBLIC_KEY` (build-time override) drifted out of sync with the
// `TAURI_SIGNING_PRIVATE_KEY` secret used to sign artifacts, every signed
// installer was rejected by its own embedded pubkey at install time. The
// static `tauri.conf.json` is now authoritative — change keys/endpoints by
// editing that file and committing, not by rotating GH secrets.
//
// What's left here is genuinely build-target-specific and can't reasonably
// live in static config: the Windows DigiCert SmartCard sign command, which
// has to interpolate `KEYPAIR_ALIAS` from the runner's secret store.
export default function prepareTauriConfig() {
  const config = {};

  if (process.env.KEYPAIR_ALIAS) {
    config.bundle = {
      windows: {
        signCommand: `smctl.exe sign --keypair-alias=${process.env.KEYPAIR_ALIAS} --input %1`,
      },
    };
  }

  return config;
}
