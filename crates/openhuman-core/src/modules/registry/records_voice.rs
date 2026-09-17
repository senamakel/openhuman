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
    version: "0.1.6",
    release_url: "https://github.com/tinyhumansai/tinyvoice/releases/tag/v0.1.6",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyvoice-module-0.1.6-ubuntu-24.04-x86_64.tar.gz",
            sha256: "40686bfb1840024d1a49bb2959e454fde44a54ec35b0e86a160fbcb031d4242a",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyvoice-module-0.1.6-ubuntu-24.04-arm64.tar.gz",
            sha256: "da691e1007691a7f4a4b377ee5f0ca03553c57f0ef887d188708d764e5b5a24b",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyvoice-module-0.1.6-ubuntu-22.04-x86_64.tar.gz",
            sha256: "b494fe15f2270718b93d1b56b1e848694ca88b6efcbdd9a134594ef042660ed7",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyvoice-module-0.1.6-ubuntu-22.04-arm64.tar.gz",
            sha256: "e641ea4eea89fa0f90f00577445351fb7a2762a6b473443c17240e5d5c6bcde9",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyvoice-module-0.1.6-macos-26-arm64.tar.gz",
            sha256: "3ff9a2d4b7b7e055cf15f4566c7b84032b90bc913ebf022cb69bc6d884fe6945",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyvoice-module-0.1.6-macos-26-x86_64.tar.gz",
            sha256: "e77f2104f3b6e6230a449d220fd1fdc475d57bc329e7ddeac6223b9cb3b50422",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyvoice-module-0.1.6-macos-15-arm64.tar.gz",
            sha256: "4ba67976463dd164471c4bbe34bf7f3ad7280fb80c4e660c6692bea62bdca20f",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyvoice-module-0.1.6-macos-15-x86_64.tar.gz",
            sha256: "39795777b8c27f8726473a8a26a6372430ac586046cd5a6c0ccc633815d83ea9",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyvoice-module-0.1.6-windows-2025-x86_64.zip",
            sha256: "22c3c48e918156ed3680e8cdcebf493f37cab99c01ec5f44ff3d316c93942c02",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyvoice-module-0.1.6-windows-2022-x86_64.zip",
            sha256: "0e7cef580404005c5736a6e5ab0a9430bc30ee017955536365dbbb2e06da5b6b",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyvoice-module-0.1.6-windows-11-arm64.zip",
            sha256: "0bc68da8a0f937436f74384184402ca502cc92d261d67b6d55f41e2e43582874",
        },
    ],
    load: LoadPolicy::Lazy,
};
