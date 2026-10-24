//! Registry record for the `tinyeventqueues` module.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset, SourceSpec};

/// The `tinyeventqueues` module: durable inbound event queues. It persists,
/// deduplicates, filters and paces inbound events (Composio triggers, agent
/// webhooks) on the host's storage, loaded on first ingest, and calls the
/// host back (`Deliver`, `ClassifySalience`) to run triage.
pub(crate) const TINYEVENTQUEUES: ModuleRecord = ModuleRecord {
    id: "tinyeventqueues",
    description: "Durable inbound event queues: dedupe, filtering, pacing and retry before triage",
    bus_name: tinyeventqueues_bus::INTERFACE,
    object_path: tinyeventqueues_bus::OBJECT_PATH,
    version: "0.1.3",
    release_url: "https://github.com/tinyhumansai/tinyeventqueues/releases/tag/v0.1.3",
    assets: &[
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyeventqueues-0.1.3-macos-15-arm64.tar.gz",
            sha256: "d58f7c1128e0e2615220f72716ec266378bca7f34bb6fbc87b9d823a217a5b71",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyeventqueues-0.1.3-macos-15-x86_64.tar.gz",
            sha256: "bc1ec0c54abab9731cdaa741ab19f7a2001661c11c32f3b4c2125b3aedc55588",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyeventqueues-0.1.3-macos-26-arm64.tar.gz",
            sha256: "610b6f03caaafa30b7e29d773ba180560fbbaa9ad92f2b60ebdf3cfa92b1a98a",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyeventqueues-0.1.3-macos-26-x86_64.tar.gz",
            sha256: "0d1f8869c83a79140f5bd6dd09f5b6ac242440521d3c115f115faea3ed132662",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyeventqueues-0.1.3-ubuntu-22.04-arm64.tar.gz",
            sha256: "66438022dc178ab604d2808454e47645cdad9b96e3ce8a05b12d291a30426a0e",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyeventqueues-0.1.3-ubuntu-22.04-x86_64.tar.gz",
            sha256: "14e2bb473fbee04863c51efb882d63dc8541f6afad05c30c23b35f54da2c638f",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyeventqueues-0.1.3-ubuntu-24.04-arm64.tar.gz",
            sha256: "843033ece85e04191b1429f5ef7015602138fb9b18493c2fae4fdacc79602364",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyeventqueues-0.1.3-ubuntu-24.04-x86_64.tar.gz",
            sha256: "abde968ef3004e17914fe08ea20ee8dbd018b6d767279d94c1c39d3f65f9a896",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyeventqueues-0.1.3-windows-11-arm64.zip",
            sha256: "f3c6a6c0d902902e54070308c5430e87dca6321cd2ef95aac629091b5d00fa12",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyeventqueues-0.1.3-windows-2022-x86_64.zip",
            sha256: "ae19a089a111eb071c92dbd6bfa30c07c3d613ec862e7e7d03646692089c1c9c",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyeventqueues-0.1.3-windows-2025-x86_64.zip",
            sha256: "936db8601ddefe34e1fa7eaa059c7b27bf0847c967e3c9415eccfe51ed9d7429",
        },
    ],
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinyeventqueues",
        manifest: "crates/tinyeventqueues/Cargo.toml",
        package: "tinyeventqueues",
        lib_name: "tinyeventqueues",
        features: &[],
    },
};
