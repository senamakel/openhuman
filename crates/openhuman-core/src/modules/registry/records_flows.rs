//! TinyFlows module: scheduled job runtime and workflow authoring through its bus contract.
use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset, SourceSpec};

pub(crate) const TINYFLOWS: ModuleRecord = ModuleRecord {
    id: "tinyflows",
    description: "Scheduled jobs, workflow Catalog/Runs execution, and authoring through TinyFlows",
    bus_name: tinyflows_bus::BUS_NAME,
    object_path: tinyflows_bus::OBJECT_PATH,
    version: "0.8.29",
    release_url: "https://github.com/tinyhumansai/tinyflows/releases/tag/v0.8.29",
    // Verbatim from the published v0.8.29 checksum.toml.
    assets: &[
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyflows-module-0.8.29-macos-26-arm64.tar.gz",
            sha256: "af1b942b4a23f764319d3378c23b3244844026f6832d7384a57a4adfc3879ca6",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyflows-module-0.8.29-macos-26-x86_64.tar.gz",
            sha256: "27916a9872608ea363421b8d4ff807c669203ee5e6546f04e33d507e2c5feb33",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyflows-module-0.8.29-macos-15-arm64.tar.gz",
            sha256: "997d5aaf21340362059edbe5f715d76c10df92823dd4c93cdfea8ad6d53c21ed",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyflows-module-0.8.29-macos-15-x86_64.tar.gz",
            sha256: "b2df10c5bf1f302eedf0466eebc44f92ef23c8fa8037b34183499b34b26ce614",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyflows-module-0.8.29-ubuntu-24.04-x86_64.tar.gz",
            sha256: "1c61b3925b366a0a9e610549249944901f90e3191be5639a9f68663af4eb4074",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyflows-module-0.8.29-ubuntu-24.04-arm64.tar.gz",
            sha256: "2ed13d6aa3e06a7bce361af9bd8ab0a2e4b2cf01d0c94fb98efd6f8a47f394c8",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyflows-module-0.8.29-ubuntu-22.04-x86_64.tar.gz",
            sha256: "d607a01766039b99e3abdd9801ee3ea41991445dd828f3a1269d982268c7a1bb",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyflows-module-0.8.29-ubuntu-22.04-arm64.tar.gz",
            sha256: "3c67ea6617b84e8b53ee7cf5dbe91cd133e5d5af6196625ab8b7db46d431ce6a",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyflows-module-0.8.29-windows-2025-x86_64.zip",
            sha256: "e26a33311840b9fbd3cbc0e2e5525a134b96a16970d14ed14f187d7110e90589",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyflows-module-0.8.29-windows-2022-x86_64.zip",
            sha256: "5f8734a5c4eeed7ab54400b896dd1f8318cc4d647954c2808b609af7b2181020",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyflows-module-0.8.29-windows-11-arm64.zip",
            sha256: "ef9b65f044489cdca520eac70104cb3f3993d2df453c47906cc9b5a1c588f371",
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
