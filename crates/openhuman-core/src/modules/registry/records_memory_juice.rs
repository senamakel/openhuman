//! Registry records for the `tinymemory` and `tinyjuice` modules.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

/// The complete TinyMemory engine, loaded eagerly so its capabilities are
/// available when the kernel assembles its RPC and tool surfaces.
pub(crate) const TINYMEMORY: ModuleRecord = ModuleRecord {
    id: "tinymemory",
    description: "Local memory engine: store, ranked recall, and portable export",
    bus_name: "ai.tinyhumans.tinymemory.Memory",
    object_path: "/ai/tinyhumans/tinymemory/Memory",
    version: "1.16.1",
    release_url: "https://github.com/tinyhumansai/tinymemory/releases/tag/v1.16.1",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinymemory-module-1.16.1-ubuntu-24.04-x86_64.tar.gz",
            sha256: "6afe17e3edd80e46538860e9445bfc45d205b8814587d49062117cb4da6edec8",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinymemory-module-1.16.1-ubuntu-24.04-arm64.tar.gz",
            sha256: "1348cd626b1ed109270ce801aeb4e68178d08daefaf79f142ccccc82bb9f1944",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinymemory-module-1.16.1-ubuntu-22.04-x86_64.tar.gz",
            sha256: "c025f4a5743bb975ed7cdc5306a16a6aa4ac2ef3c87fa83820d038321123daee",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinymemory-module-1.16.1-ubuntu-22.04-arm64.tar.gz",
            sha256: "27bc9694b468b7d7597e3259945148ce6fe39fec4f558dac5de1a554b1508169",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinymemory-module-1.16.1-macos-26-arm64.tar.gz",
            sha256: "dcd45c7e030ef01d6a48174c57aea48adfebb221144fb579c18180f068ec13df",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinymemory-module-1.16.1-macos-26-x86_64.tar.gz",
            sha256: "e2cdc685ecb24ad83ab98a1ce88be4b2e563535219eee8609a91ec1bcc5ac490",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinymemory-module-1.16.1-macos-15-arm64.tar.gz",
            sha256: "fd3c002707011ab594b3ac73114230aed570a6ebc9215e7c915d3024b74d8a19",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinymemory-module-1.16.1-macos-15-x86_64.tar.gz",
            sha256: "63030e318b20a410ea5f0dc8addd6c76a7f1848a45c8badb4e2c96f3f8a0539a",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinymemory-module-1.16.1-windows-2025-x86_64.zip",
            sha256: "94f902a44928d4485a62b5c7531dd91985faa3da466748777d8f1c0f3227eeaa",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinymemory-module-1.16.1-windows-2022-x86_64.zip",
            sha256: "d0f1aea80b3ee1b96eeeb1663b45ffdfd5ca32e09ce9b8653864c8fafc49e0b5",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinymemory-module-1.16.1-windows-11-arm64.zip",
            sha256: "3c287618965a203d1165490ca05693315e523008ad94acaafacb6c2dc998b041",
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
    version: "0.3.3",
    release_url: "https://github.com/tinyhumansai/tinyjuice/releases/tag/v0.3.3",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyjuice-module-0.3.3-ubuntu-24.04-x86_64.tar.gz",
            sha256: "22974edf316dbde280bab7885be06c20b07a6dc35bb02a75e0865aab999c65f1",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyjuice-module-0.3.3-ubuntu-24.04-arm64.tar.gz",
            sha256: "5db1a3261827eb391fdcab00592b48f5f02206738dd731187409cd21c6b9d6a9",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyjuice-module-0.3.3-ubuntu-22.04-x86_64.tar.gz",
            sha256: "aa3cabf8248a7aa84bdf257f5a4f16c2f4f69e680a135ba96d06f89fd78d2725",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyjuice-module-0.3.3-ubuntu-22.04-arm64.tar.gz",
            sha256: "15b99c36a108db62be5a04fd92ca074a394902d32b2d86f021ab42a41efb7814",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyjuice-module-0.3.3-macos-26-arm64.tar.gz",
            sha256: "74026227556bd96234e2e7990937ef13327f74afa2ca371dd0c4077a83d24d44",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyjuice-module-0.3.3-macos-26-x86_64.tar.gz",
            sha256: "345eff0f44190fc8f1921bc8e34cc49789d17ef69abe23fdcde63f13da6f73e2",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyjuice-module-0.3.3-macos-15-arm64.tar.gz",
            sha256: "8e1a29d65442bb2ede325cc8ba739fbb5629d3fbd18031efd8b5d501be4e00ce",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyjuice-module-0.3.3-macos-15-x86_64.tar.gz",
            sha256: "15d492f7dab844e294ede11140ec0bc1b62dfa6cb8d94bc3da39da795e0da391",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyjuice-module-0.3.3-windows-2025-x86_64.zip",
            sha256: "2d6daa3f13bcb1b1b7eaa3cc4ffaeda81a1ca1f62e9a5b265be9b707a7d60826",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyjuice-module-0.3.3-windows-2022-x86_64.zip",
            sha256: "1134eb44c13c6e331b68b91796681a45074483b222817ceb14d5b0e48edb49bb",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyjuice-module-0.3.3-windows-11-arm64.zip",
            sha256: "213f6029d45aad27a69c000865393c6c1d698a205a61d921a3ad92583ae5acbf",
        },
    ],
    load: LoadPolicy::Lazy,
};
