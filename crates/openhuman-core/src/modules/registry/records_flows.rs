//! TinyFlows module: scheduled job runtime and workflow authoring through its bus contract.
use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset, SourceSpec};

pub(crate) const TINYFLOWS: ModuleRecord = ModuleRecord {
    id: "tinyflows",
    description: "Scheduled jobs, workflow Catalog/Runs execution, and authoring through TinyFlows",
    bus_name: tinyflows_bus::BUS_NAME,
    object_path: tinyflows_bus::OBJECT_PATH,
    version: "0.8.28",
    release_url: "https://github.com/tinyhumansai/tinyflows/releases/tag/v0.8.28",
    // Verbatim from the published v0.8.28 checksum.toml.
    assets: &[
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyflows-module-0.8.28-macos-26-arm64.tar.gz",
            sha256: "387b2fdf527b087700cc7b68fee2866fe5590a34f5041819793b5b05caf97e90",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyflows-module-0.8.28-macos-26-x86_64.tar.gz",
            sha256: "fdcba79686d466ec2d163ab23ed0a56e148884c360b66a074c0dbb918decb6a3",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyflows-module-0.8.28-macos-15-arm64.tar.gz",
            sha256: "923d720d622a535e6cd547ffda1c6f73cc70b7b1c13681a374633d81f51069eb",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyflows-module-0.8.28-macos-15-x86_64.tar.gz",
            sha256: "d2f34c52863cb1d3a78256f59fa9e54e26af62b39540f24f072c89e18de6ae65",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyflows-module-0.8.28-ubuntu-24.04-x86_64.tar.gz",
            sha256: "c3224ba59b7683cf2459a1e1e8946b5ad2c6200d729c96bfd9c084450c6cd65d",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyflows-module-0.8.28-ubuntu-24.04-arm64.tar.gz",
            sha256: "2306e7fda0f1edb75e90b79bd645ec1806f04d004b5ec615c83f67789c8a3655",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyflows-module-0.8.28-ubuntu-22.04-x86_64.tar.gz",
            sha256: "54bb2ac4e085cac962d368815d485df3e0050b8b475a66b58cd96a044c4de40e",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyflows-module-0.8.28-ubuntu-22.04-arm64.tar.gz",
            sha256: "3d92636d08b549533452ac971c01edb42a02be170ea974286c215647c09f3491",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyflows-module-0.8.28-windows-2025-x86_64.zip",
            sha256: "d8772f00d1dce3ff6000cee492348bdb8176660d798fa55edecf172c3d279ef0",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyflows-module-0.8.28-windows-2022-x86_64.zip",
            sha256: "e1c926be3b5f1e8d4b2d1492797df2357ddfb3d90fdeb190664fe972f9703ddf",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyflows-module-0.8.28-windows-11-arm64.zip",
            sha256: "8395463124c85256009fae44e0b34c95b3a494270bbc7f8e80cc03d4f174fd9a",
        },
    ],
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinyflows",
        manifest: "crates/tinyflows-module/Cargo.toml",
        package: "tinyflows-module",
        lib_name: "tinyflows_module",
        features: &[],
    },
};
