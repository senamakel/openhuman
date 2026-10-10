//! Registry record for the `tinyvoice` module.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

/// The `tinyvoice` module: the host-agnostic half of the voice pipeline.
///
/// Wake-word gating, fast-path command routing, STT hallucination detection,
/// and the capture-side audio work (downmix, resample, silence gate, WAV
/// framing).
///
/// Lazy, and more clearly so than the others: voice is opt-in twice over — a
/// user has to enable dictation or always-on listening before any of this runs
/// — so a session that never speaks should not pay a download or a `dlopen`.
///
/// **The VAD deliberately does not come through here.** A segmenter is driven
/// once per 20 ms frame from inside a `cpal` callback, and a bus round trip at
/// that cadence would cost more than the sixty-line state machine it replaces.
/// `voice::always_on` keeps its own; see [`super::voice`].
pub(crate) const TINYVOICE: ModuleRecord = ModuleRecord {
    id: "tinyvoice",
    description: "Wake-word gating, command routing, hallucination detection, capture audio",
    bus_name: "ai.tinyhumans.tinyvoice.Voice",
    object_path: "/ai/tinyhumans/tinyvoice/Voice",
    version: "0.1.12",
    release_url: "https://github.com/tinyhumansai/tinyvoice/releases/tag/v0.1.12",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyvoice-module-0.1.12-ubuntu-24.04-x86_64.tar.gz",
            sha256: "7aa0efa8ca6bd9553c7c318f1c96d7535917fbdf7f4e73e6af0bfe69fac42f67",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyvoice-module-0.1.12-ubuntu-24.04-arm64.tar.gz",
            sha256: "0035bd13161dca209ee96fc971025c0387fc58d30ef9d5c79ac8e4b0e522ff9e",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyvoice-module-0.1.12-ubuntu-22.04-x86_64.tar.gz",
            sha256: "ff201a53f4370c79c0ea70cb3649641f48f941022db86ae8f382098ec88c0f27",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyvoice-module-0.1.12-ubuntu-22.04-arm64.tar.gz",
            sha256: "d49fba688acf2779b551c2799f0fecd2afe4fe1ce72c5c37980a51999eb0eb55",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyvoice-module-0.1.12-macos-26-arm64.tar.gz",
            sha256: "88bcaefbbef19a208b7c7c43739f2f39081a44f335105bdd4ce85b54945e8ffb",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyvoice-module-0.1.12-macos-26-x86_64.tar.gz",
            sha256: "32054a8c995106cb7af21b4fc2b66772b67b85b41a280d457337480ecab4d395",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyvoice-module-0.1.12-macos-15-arm64.tar.gz",
            sha256: "1444a6dda431b83ab719ecc0d098b58af8ede42f652ed806fb259968d94ce9c1",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyvoice-module-0.1.12-macos-15-x86_64.tar.gz",
            sha256: "0e2a3f11e333324837748b55562a64279cb01d33826a07f2258b8fb1fedfbee3",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyvoice-module-0.1.12-windows-2025-x86_64.zip",
            sha256: "475389e407754c3c73adb5e61bc3113b9538e50c50e71c23ea5f92d6b934ccd8",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyvoice-module-0.1.12-windows-2022-x86_64.zip",
            sha256: "43149836ee514f27fd5f639cd7b84d287df689cc8658c45e1a41d6960fbfc387",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyvoice-module-0.1.12-windows-11-arm64.zip",
            sha256: "d11e59f3e16aab98f699edb37e3d6716469f278a961d39794f832000296be315",
        },
    ],
    load: LoadPolicy::Lazy,
};
