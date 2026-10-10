//! Registry record for the `tinyjuice` module.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

pub(crate) const TINYJUICE: ModuleRecord = ModuleRecord {
    id: "tinyjuice",
    description: "Content-aware tool-output compression and recoverable caching",
    bus_name: "ai.tinyhumans.tinyjuice.Compression",
    object_path: "/ai/tinyhumans/tinyjuice/Compression",
    version: "0.6.1",
    release_url: "https://github.com/tinyhumansai/tinyjuice/releases/tag/v0.6.1",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyjuice-module-0.6.1-ubuntu-24.04-x86_64.tar.gz",
            sha256: "cdb6e564bb531b8c43a6a5250cedb34268f9623787502ad9fdcce2b331af5bb2",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyjuice-module-0.6.1-ubuntu-24.04-arm64.tar.gz",
            sha256: "e3f55125b1367eeaba39987f6d80bdf0da2c4f18da57b6f6afc52ec21a40b18b",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyjuice-module-0.6.1-ubuntu-22.04-x86_64.tar.gz",
            sha256: "a85f68188c49b853184c26f81a9aa888a3367271a3e11c13e4dd2a7401abb594",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyjuice-module-0.6.1-ubuntu-22.04-arm64.tar.gz",
            sha256: "aaa238f0b27e2cadcf0aabbde029b9aa896a75a837d490d3c9b4400c467e183b",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyjuice-module-0.6.1-macos-26-arm64.tar.gz",
            sha256: "d4a16acb408cd9ec9d4bde065cc9a1acc67bd2177a6eb90b0f90377ac2063ccc",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyjuice-module-0.6.1-macos-26-x86_64.tar.gz",
            sha256: "f5993a5cfab65beac1de8e93ecbbe17e0239d7d8255f4d0194beaa7af99630b2",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyjuice-module-0.6.1-macos-15-arm64.tar.gz",
            sha256: "b04f48708fa78c196f98c2ab692343acac547aa8b361039ee88c8fc64f49d9d6",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyjuice-module-0.6.1-macos-15-x86_64.tar.gz",
            sha256: "bb4ed1fc271958fd47c8af82387307e7fefa7d7945073a54ec0c62587367b07c",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyjuice-module-0.6.1-windows-2025-x86_64.zip",
            sha256: "4f821817706a3064c3894cfd9f677cc6841e8a3271d134d7c2894c1a4c426b90",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyjuice-module-0.6.1-windows-2022-x86_64.zip",
            sha256: "a29ef60e7959a416d548bba077cbd3747dacce286e72c5ad4eda6042f7557da3",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyjuice-module-0.6.1-windows-11-arm64.zip",
            sha256: "bcdc2d8ba0770ba1c68356a825786cfc0d2ad1316c31f2a3a1edd3045190c3f7",
        },
    ],
    load: LoadPolicy::Lazy,
};
