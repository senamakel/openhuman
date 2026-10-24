//! Registry records for additional first-party TinyBus modules.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset, SourceSpec};

/// The `tinybox` module, loaded on demand.
#[cfg(feature = "modules")]
pub(crate) const TINYBOX: ModuleRecord = ModuleRecord {
    id: "tinybox",
    description: "Sandbox execution and lifecycle operations through TinyBox",
    bus_name: tinybox_bus::INTERFACE,
    object_path: tinybox_bus::OBJECT_PATH,
    version: "0.1.42",
    release_url: "https://github.com/tinyhumansai/tinybox/releases/tag/v0.1.42",
    assets: &[
        PlatformAsset {
            host_key: "archlinux-arm64",
            archive: "tinybox-0.1.42-archlinux-arm64.tar.gz",
            sha256: "5f276b8b3bc291710619775e5a8dffea3c65010040f8782c9e34ba0d2bc09fe6",
        },
        PlatformAsset {
            host_key: "archlinux-x86_64",
            archive: "tinybox-0.1.42-archlinux-x86_64.tar.gz",
            sha256: "570b2a0c693a3404e085c633ed9085833131d256c08ea32afddfcb0c6229ae72",
        },
        PlatformAsset {
            host_key: "fedora-43-arm64",
            archive: "tinybox-0.1.42-fedora-43-arm64.tar.gz",
            sha256: "1c19f32de81571691aca6c72cf8e23cda3f15a96849204f0b0ec2f19b5e95c52",
        },
        PlatformAsset {
            host_key: "fedora-43-x86_64",
            archive: "tinybox-0.1.42-fedora-43-x86_64.tar.gz",
            sha256: "876d48362e1384b805fc80eda6bdd5840ae265f812d2a058e74bf6c6d7ce633d",
        },
        PlatformAsset {
            host_key: "fedora-44-arm64",
            archive: "tinybox-0.1.42-fedora-44-arm64.tar.gz",
            sha256: "cb765100fa700d751c51746697d499005eb1338dbe5d0aaf3a2cc70975eb35e2",
        },
        PlatformAsset {
            host_key: "fedora-44-x86_64",
            archive: "tinybox-0.1.42-fedora-44-x86_64.tar.gz",
            sha256: "d3ec5b0ab0b5bafb9ecc61701997aa1bfb84d5c4afdff64aa90c4500a087d030",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinybox-0.1.42-macos-15-arm64.tar.gz",
            sha256: "d87fec4edace1f14514b46d4a442d8a3e124eeb8890cc1dcf851fcd4e663c257",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinybox-0.1.42-macos-15-x86_64.tar.gz",
            sha256: "aa5b5920e5463db8fc8b891c81bd1695de67373809b93aa331c2ba5800309b90",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinybox-0.1.42-macos-26-arm64.tar.gz",
            sha256: "1a06b12a46cbd48b8bd8ff2ef3a90b3df2421df8e13e0541ce12dea7dac21345",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinybox-0.1.42-macos-26-x86_64.tar.gz",
            sha256: "0efcbf868c59e8ba933fc745bc8bb51cb5052f6574de4f22df50195a45ba65a4",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinybox-0.1.42-ubuntu-22.04-arm64.tar.gz",
            sha256: "9876bc55ceca33cceebeff78e9e81fb6edc3a8b1a876c71c727cce942833ead9",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinybox-0.1.42-ubuntu-22.04-x86_64.tar.gz",
            sha256: "63c40e2577b97ce0ba369657a153e725a8db89b0043abfc3cb874075c266ecd5",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinybox-0.1.42-ubuntu-24.04-arm64.tar.gz",
            sha256: "58b533cd33ed5d3ef83814dbde6a9e417bb83f2087a9717f1a98a0babdac6c7b",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinybox-0.1.42-ubuntu-24.04-x86_64.tar.gz",
            sha256: "2dcc913153c5c6a03b88cc41e8aa14151159b308799b9174eb9e1a27edf1cf36",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinybox-0.1.42-windows-11-arm64.zip",
            sha256: "19e781b93bcb0d401ff6b9349b62bfd00d05f92e82875eed36d0689d3484692b",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinybox-0.1.42-windows-2022-x86_64.zip",
            sha256: "e9f8cc22dfce616bdf57af076ca33390c37f1417b56a26d58d265b00564ee52d",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinybox-0.1.42-windows-2025-x86_64.zip",
            sha256: "8ace9208d6a89a6d4d2766a91d5f9be937e1aaf076b4a7ca3af5c965997167d3",
        },
    ],
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinybox",
        manifest: "crates/tinybox-module/Cargo.toml",
        package: "tinybox-module",
        lib_name: "tinybox",
        features: &[],
    },
};

/// The `tinychannels` module, loaded on demand.
pub(crate) const TINYCHANNELS: ModuleRecord = ModuleRecord {
    id: "tinychannels",
    description: "Channel provider lifecycle and message transport",
    bus_name: tinychannels_bus::names::BUS_NAME,
    object_path: tinychannels_bus::names::OBJECT_PATH,
    version: "0.1.40",
    release_url: "https://github.com/tinyhumansai/tinychannels/releases/tag/v0.1.40",
    assets: &[
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinychannels-module-0.1.40-macos-15-arm64.tar.gz",
            sha256: "98d31e633601c365200c679f288b13f9d7aef2023ebb9853bcbad690ddbbe1a0",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinychannels-module-0.1.40-macos-15-x86_64.tar.gz",
            sha256: "4ac740b410102b29ba469f37f450b3e03fac3ce31ccb9d3538c1633025a7bd4d",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinychannels-module-0.1.40-macos-26-arm64.tar.gz",
            sha256: "ed2aab10a198193e5bebb5957095acad664a5a66a02d6142563815df6c353b53",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinychannels-module-0.1.40-macos-26-x86_64.tar.gz",
            sha256: "210a7904dd942629af46d7ad36d0b0312f3a1cf0aa238b667ca6309ebf86c8e2",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinychannels-module-0.1.40-ubuntu-22.04-arm64.tar.gz",
            sha256: "48d360aea7849bb93050dbb56f9c28de6848572632908b635fb77df2a65c3bb1",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinychannels-module-0.1.40-ubuntu-22.04-x86_64.tar.gz",
            sha256: "6026081ac8ceb7b91daf0101364c92f99c10c5072a270b91eaeed6acad5d1f90",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinychannels-module-0.1.40-ubuntu-24.04-arm64.tar.gz",
            sha256: "0b4ef6b3c9c2c81f1efdbc5cbe8713478ff4e2f76682625465ee8c7120dfb798",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinychannels-module-0.1.40-ubuntu-24.04-x86_64.tar.gz",
            sha256: "bec22856f0b5f36f8c48f893c53d83015961a596d4ad56a33714ca378c2b4c98",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinychannels-module-0.1.40-windows-11-arm64.zip",
            sha256: "5f463711be6f8c856e13f77ba5d376b1b4e5b9519551411ae7bfd94a196bd633",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinychannels-module-0.1.40-windows-2022-x86_64.zip",
            sha256: "5df4ce49573c7d1e03a93b50cf092bf62dc6b92f50bcfe1471aff50a844268ce",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinychannels-module-0.1.40-windows-2025-x86_64.zip",
            sha256: "e8dd10cc102a7889d7d9ba8f50416b2ee3cec810f203555bf40cd0f157765552",
        },
    ],
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinychannels",
        manifest: "crates/tinychannels-module/Cargo.toml",
        package: "tinychannels-module",
        lib_name: "tinychannels_module",
        features: &[],
    },
};

#[cfg(feature = "hosting")]
/// The `tinyhosts` module, loaded on demand.
pub(crate) const TINYHOSTS: ModuleRecord = ModuleRecord {
    id: "tinyhosts",
    description: "Hosting provider operations",
    bus_name: tinyhosts_bus::BUS_NAME,
    object_path: tinyhosts_bus::OBJECT_PATH,
    version: "0.4.1",
    release_url: "https://github.com/tinyhumansai/tinyhosts/releases/tag/v0.4.1",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyhosts-0.4.1-ubuntu-24.04-x86_64.tar.gz",
            sha256: "7e8a72b7951db71cc98c94fed0908660a58bd3f2cbf23103d953e28e1315d4d5",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyhosts-0.4.1-ubuntu-24.04-arm64.tar.gz",
            sha256: "27cbc5a233292850797adc7b51d9d654a10d4aa77ec285a7e564e378889e0a03",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyhosts-0.4.1-ubuntu-22.04-x86_64.tar.gz",
            sha256: "a03588ef8b012aea29f2b2b7105fde6c3c7b4bb0dc6f93d99868e91468c2b6d6",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyhosts-0.4.1-ubuntu-22.04-arm64.tar.gz",
            sha256: "80b26591eb7beff4e27de9e6239b919ff6c17b00ead11d22bae8e7fcb0d33c15",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyhosts-0.4.1-macos-26-arm64.tar.gz",
            sha256: "ba40e889ebd450edd791865945b4146b2f243bc885f4228463e65b0a4ea8074e",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyhosts-0.4.1-macos-26-x86_64.tar.gz",
            sha256: "eeb2a66603374934724036e9fb86fcba0c22a2fd3aa168127cca5960eb5a19aa",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyhosts-0.4.1-macos-15-arm64.tar.gz",
            sha256: "b67a0aecb0527af68d5b5428d55d897ece11500e2ee9798b3d133ef0df7344b7",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyhosts-0.4.1-macos-15-x86_64.tar.gz",
            sha256: "7582dc327e247b28e657a01c8f8a4cb0924c8211144dfb24e4917868cd237354",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyhosts-0.4.1-windows-2025-x86_64.zip",
            sha256: "42c2a5b98eb5c51a8a05731e762af801c585383024d27cf4cd8bb01f9014308f",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyhosts-0.4.1-windows-2022-x86_64.zip",
            sha256: "0d579e0d08856307881b8965bf78762f0064b129861e13e796f91fcb9895ccaf",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyhosts-0.4.1-windows-11-arm64.zip",
            sha256: "67b64f0917fa0cf561aa08dfe0c74818e9ac9066a9f03c03e6b216f7af87aea3",
        },
    ],
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinyhosts",
        manifest: "Cargo.toml",
        package: "tinyhosts",
        lib_name: "tinyhosts",
        features: &[],
    },
};
