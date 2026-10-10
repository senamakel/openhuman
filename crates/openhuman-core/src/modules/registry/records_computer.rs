//! Native computer-use module (desktop and browser control). Published digests are added only from its release manifest.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};
use tinycomputer_bus::names;

pub(crate) const TINYCOMPUTER: ModuleRecord = ModuleRecord {
    id: "tinycomputer",
    description: "Permission-aware desktop and browser observation and control",
    bus_name: names::INTERFACE,
    object_path: names::OBJECT_PATH,
    version: "0.10.1",
    release_url: "https://github.com/tinyhumansai/tinycomputer/releases/tag/v0.10.1",
    // Verbatim from the published tinycomputer v0.10.1 checksum.toml. Linux remains outside
    // the initial product surface; this registry admits macOS and Windows.
    assets: &[
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinycomputer-0.10.1-macos-26-arm64.tar.gz",
            sha256: "39d1ca3db737c26b4d39d9a22c16da7845735c6349dd012c59533499498af874",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinycomputer-0.10.1-macos-26-x86_64.tar.gz",
            sha256: "b135b0885cc8d8eaff250d6fb781f7f9e1c8bdbf86dacf2dc44a291c18a1d9ae",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinycomputer-0.10.1-macos-15-arm64.tar.gz",
            sha256: "14d55317c96dc0a89bc66b1248da68096d81f0a39b5a7ab8a286db8bbba55368",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinycomputer-0.10.1-macos-15-x86_64.tar.gz",
            sha256: "c8694118a5e8ac11f3785acda9023044c443bf5ae820695fce17b8c72af00fd0",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinycomputer-0.10.1-windows-2025-x86_64.zip",
            sha256: "91315b4cefc5a4d034e43d8d5fa287fbac3cb80a74ada4da842c1569b203516f",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinycomputer-0.10.1-windows-2022-x86_64.zip",
            sha256: "5907acb6d46e8ea33ef5024bf8f5586d17259db92560990a9e320aae9a5e3bbf",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinycomputer-0.10.1-windows-11-arm64.zip",
            sha256: "71a7052fcb5cd70bf2ce8a41996978177882a2a41a2601f746bce519fa1fc642",
        },
    ],
    load: LoadPolicy::Lazy,
};
