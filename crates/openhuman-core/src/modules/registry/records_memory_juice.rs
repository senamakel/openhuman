//! Registry records for the `tinymemory` and `tinyjuice` modules.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

/// The complete TinyMemory engine, loaded eagerly so its capabilities are
/// available when the kernel assembles its RPC and tool surfaces.
pub(crate) const TINYMEMORY: ModuleRecord = ModuleRecord {
    id: "tinymemory",
    description: "Local memory engine: store, ranked recall, and portable export",
    bus_name: "ai.tinyhumans.tinymemory.Memory",
    object_path: "/ai/tinyhumans/tinymemory/Memory",
    version: "1.16.0",
    release_url: "https://github.com/tinyhumansai/tinymemory/releases/tag/v1.16.0",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinymemory-module-1.16.0-ubuntu-24.04-x86_64.tar.gz",
            sha256: "fc65fce075b0d286b0d1cce48fc8952c2936e4b945f2af84b6a20d800c2a10d7",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinymemory-module-1.16.0-ubuntu-24.04-arm64.tar.gz",
            sha256: "1735efb7b0b6a56c85da1b62fbfa2d2a68995a04203ec2d79b4e1b389935fd92",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinymemory-module-1.16.0-ubuntu-22.04-x86_64.tar.gz",
            sha256: "f3ba06867ec89b8374a405f8a8569f5cecf88490609354ae4a6c1faa6e55b425",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinymemory-module-1.16.0-ubuntu-22.04-arm64.tar.gz",
            sha256: "b9f6794806e9463cffbfc3ce0c4b4b39cb8c1f3403a2ba956b0984cc35e9354a",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinymemory-module-1.16.0-macos-26-arm64.tar.gz",
            sha256: "097ab5fdc352f84f34b770302db73c5c1468e7ad48709e1767e74d61cc55646c",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinymemory-module-1.16.0-macos-26-x86_64.tar.gz",
            sha256: "3128eaff5e820c86fabfbe6a759976150313ca3c2c38e00bb2bb67dd2dfa76ba",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinymemory-module-1.16.0-macos-15-arm64.tar.gz",
            sha256: "fa9d65f31b7ece3eed0d118795ce06b66f0bd271556b91ccad09b0c4b41d94f9",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinymemory-module-1.16.0-macos-15-x86_64.tar.gz",
            sha256: "ac1343f128cdd4b299b43318019d1b26c0c8fc28abf62bf24ff8fb6d9894730f",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinymemory-module-1.16.0-windows-2025-x86_64.zip",
            sha256: "dcf5ec88583a02b284f0b430a037230a3d974703d39f11be53a4e36688c8d7db",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinymemory-module-1.16.0-windows-2022-x86_64.zip",
            sha256: "cf56453fba522a075e569d60c39d7e56e938ff2d4fbb8270debaffd7e6b39819",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinymemory-module-1.16.0-windows-11-arm64.zip",
            sha256: "9d0ca43cc3c7cac29e631e9870fc30d3fd4971e272e393ed2a91adb7ca20fdf3",
        },
    ],
    // Eager, unlike the two codecs above. A codec that is never asked for should
    // not be paid for, but a memory driver's absence changes what the kernel
    // offers rather than merely delaying it: capabilities are read at bind time
    // and the RPC surface and agent-tool list are filtered from them. Resolving
    // that during a user's first recall would mean the first recall is the one
    // that behaves differently.
    load: LoadPolicy::Eager,
};

/// The `tinyjuice` content-aware tool-output compression engine.
///
/// Lazy because the host's compaction policy can disable it, and a session that
/// never produces compressible tool output should not pay the download or
/// resident native-library cost.
pub(crate) const TINYJUICE: ModuleRecord = ModuleRecord {
    id: "tinyjuice",
    description: "Content-aware tool-output compression and recoverable caching",
    bus_name: "ai.tinyhumans.tinyjuice.Compression",
    object_path: "/ai/tinyhumans/tinyjuice/Compression",
    version: "0.2.5",
    release_url: "https://github.com/tinyhumansai/tinyjuice/releases/tag/v0.2.5",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyjuice-module-0.2.5-ubuntu-24.04-x86_64.tar.gz",
            sha256: "e46e1b9338c20ce3b42403ace7bd7fe563a553ea3472f2111292a3fe200f6e67",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyjuice-module-0.2.5-ubuntu-24.04-arm64.tar.gz",
            sha256: "fb2098d392d37001af728263fd375ab4663bc301be45e8ce4c713ee2b070badf",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyjuice-module-0.2.5-ubuntu-22.04-x86_64.tar.gz",
            sha256: "946b78e860a1d8c913d83b913daef4966cfccaa2eb8814d4107bbbd63813033d",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyjuice-module-0.2.5-ubuntu-22.04-arm64.tar.gz",
            sha256: "6eaae41654850e1fa695f3bbba6eb2c9531cd97eb3e5f163d8290510f3e668e5",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyjuice-module-0.2.5-macos-26-arm64.tar.gz",
            sha256: "c1c154619827ca8c9f4f7b73c05dbbf992e98242de5e46bfe15dd82aa1c21746",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyjuice-module-0.2.5-macos-26-x86_64.tar.gz",
            sha256: "b02842c52ef0f92849d08d839a01096a4b9b2900e364f4bd4f32076c2c4d2bad",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyjuice-module-0.2.5-macos-15-arm64.tar.gz",
            sha256: "f22a74aaf6972b7062ceb83ce79671a80f26135a2ee507a74a9c250dcf6bf250",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyjuice-module-0.2.5-macos-15-x86_64.tar.gz",
            sha256: "f0b428fac0c8a7352faf284e27ba73fab949686b404373be9792ce0fa40471eb",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyjuice-module-0.2.5-windows-2025-x86_64.zip",
            sha256: "0c37efc18feeaf84388d9a818ea821c5b92a937ae49f5fd2d2ed7e7436615e1e",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyjuice-module-0.2.5-windows-2022-x86_64.zip",
            sha256: "56685bba88c800f7bfb0d6a61dba1d4c31f63ae6eb8ad9df12116a39739a3df2",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyjuice-module-0.2.5-windows-11-arm64.zip",
            sha256: "3c0956cb1e7f0123b0a53c8f7e1f66a7eb672b090d5a8add611c29844726331c",
        },
    ],
    load: LoadPolicy::Lazy,
};
