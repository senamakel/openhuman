use std::env;
use std::path::PathBuf;

fn main() {
    maybe_override_tauri_config_for_local_builds();
    tauri_build::build();
}

fn maybe_override_tauri_config_for_local_builds() {
    let profile = env::var("PROFILE").unwrap_or_default();
    let skip_resources = env::var("TAURI_SKIP_RESOURCES").is_ok() || profile == "test";
    let is_release = profile == "release";
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let tdlib_framework_path = manifest_dir.join("libraries/libtdjson.1.8.29.dylib");
    let skip_missing_frameworks = !is_release && !tdlib_framework_path.exists();

    if !skip_resources && !skip_missing_frameworks {
        return;
    }

    let mut merge_config = serde_json::json!({});
    if skip_resources {
        merge_config["bundle"]["resources"] = serde_json::json!([]);
    }
    if skip_missing_frameworks {
        merge_config["bundle"]["macOS"]["frameworks"] = serde_json::json!([]);
    }

    match serde_json::to_string(&merge_config) {
        Ok(json) => {
            env::set_var("TAURI_CONFIG", json);
            if skip_resources {
                println!("cargo:warning=TAURI resources disabled for local build");
            }
            if skip_missing_frameworks {
                println!(
                    "cargo:warning=TAURI macOS frameworks disabled because {} is missing",
                    tdlib_framework_path.display()
                );
            }
        }
        Err(err) => {
            println!("cargo:warning=Failed to serialize TAURI_CONFIG override: {err}");
        }
    }
}

