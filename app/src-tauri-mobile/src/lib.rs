// OpenHuman mobile (iOS) Tauri host.
//
// No CEF runtime, no Rust core sidecar, no desktop chrome. The React app
// (built from `app/src/`) is loaded into a single WKWebView; it talks to a
// remote desktop core via the TS-side TransportManager (LAN HTTP / encrypted
// tunnel / cloud HTTP — see `app/src/services/transport/`).

#[cfg(not(target_os = "ios"))]
compile_error!("openhuman-mobile only supports iOS. Use app/src-tauri for desktop.");

use tauri::{AppHandle, Manager, RunEvent};

/// Tauri command: terminate the iOS app cleanly. The Settings panel uses this
/// to back out of a session; without it, only the iOS task switcher can quit
/// the process.
#[tauri::command]
async fn app_quit(app: AppHandle) -> Result<(), String> {
    log::info!("[mobile] app_quit invoked");
    app.exit(0);
    Ok(())
}

pub fn run() {
    log::info!("[mobile] run() — starting iOS Tauri builder");

    tauri::Builder::default()
        .plugin(tauri_plugin_barcode_scanner::init())
        .plugin(tauri_plugin_ptt::init())
        .invoke_handler(tauri::generate_handler![app_quit])
        .setup(|app| {
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.show();
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application for iOS")
        .run(|_app_handle, event| {
            if let RunEvent::Exit = event {
                log::info!("[mobile] RunEvent::Exit");
            }
        });
}
