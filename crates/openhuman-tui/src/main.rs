//! Binary entry point for the standalone terminal frontend; all logic lives in
//! the `openhuman_tui` library (see `lib.rs` and `README.md`).

fn main() {
    // Keep the guard alive for the entire terminal session. `init_for_tui`
    // installs the tracing layer, while this creates the client that receives
    // its events and installs Sentry's panic integration.
    let _sentry_guard = openhuman_tui::init_crash_reporting();
    if let Err(error) = openhuman_tui::run_from_cli(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}
