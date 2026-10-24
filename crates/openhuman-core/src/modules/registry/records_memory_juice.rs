//! Registry record for the `tinyjuice` module.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset, SourceSpec};

pub(crate) const TINYJUICE: ModuleRecord = ModuleRecord {
    id: "tinyjuice",
    description: "Content-aware tool-output compression and recoverable caching",
    bus_name: tinyjuice_bus::names::BUS_NAME,
    object_path: tinyjuice_bus::names::OBJECT_PATH,
    version: "0.11.3",
    release_url: "https://github.com/tinyhumansai/tinyjuice/releases/tag/v0.11.3",
    assets: &[
        PlatformAsset {
            host_key: "archlinux-arm64",
            archive: "tinyjuice-module-0.11.3-archlinux-arm64.tar.gz",
            sha256: "a6c88a69ab9934b7448b7ffcbe0fb2e2ae6b4da982014462af387a6753dcca1c",
        },
        PlatformAsset {
            host_key: "archlinux-x86_64",
            archive: "tinyjuice-module-0.11.3-archlinux-x86_64.tar.gz",
            sha256: "8676de949a7ec01f724d2e21fae4c421548d4b222cab85931246898137e578dc",
        },
        PlatformAsset {
            host_key: "fedora-43-arm64",
            archive: "tinyjuice-module-0.11.3-fedora-43-arm64.tar.gz",
            sha256: "923798faecf8c410c1bca5ad1e3e9a294f77466faee9e493e194b8c5cee22929",
        },
        PlatformAsset {
            host_key: "fedora-43-x86_64",
            archive: "tinyjuice-module-0.11.3-fedora-43-x86_64.tar.gz",
            sha256: "6f146e314d0a1f11a3cc81afdcb624c6858a2890e720301a602a5052c679a00b",
        },
        PlatformAsset {
            host_key: "fedora-44-arm64",
            archive: "tinyjuice-module-0.11.3-fedora-44-arm64.tar.gz",
            sha256: "9cdc3023f287b4bd8e94f497cdea3dad40f06dfe455ecef9b48a15a9ec18d908",
        },
        PlatformAsset {
            host_key: "fedora-44-x86_64",
            archive: "tinyjuice-module-0.11.3-fedora-44-x86_64.tar.gz",
            sha256: "491c6414e864f675b19639d55995d3976c22a285c719c1e575eaf42fb0f814d9",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyjuice-module-0.11.3-macos-15-arm64.tar.gz",
            sha256: "da905dde35169b096d40358a305c5b2d9545fe91363602f643aa178ade16f48e",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyjuice-module-0.11.3-macos-15-x86_64.tar.gz",
            sha256: "f30bc49071e1c0ff350839a11dd5637bb760805dafc2f652ff308924f7969648",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyjuice-module-0.11.3-macos-26-arm64.tar.gz",
            sha256: "a857c21cd643b93184d9a4a89dd3cd5b806ae6d19b3a4c8ae6f4d9af2e795a4e",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyjuice-module-0.11.3-macos-26-x86_64.tar.gz",
            sha256: "d62a92f5b46a658a5fb63a02d3a14385563042a777d5c3bc58dfa355023a306b",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyjuice-module-0.11.3-ubuntu-22.04-arm64.tar.gz",
            sha256: "1f5f9084b7d7f7053988f3b56b94f8aca68d2790a8dc01ce89bb2973f08d731d",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyjuice-module-0.11.3-ubuntu-22.04-x86_64.tar.gz",
            sha256: "85a1e3a4ae2c35a33f8ace397098bb416da3603d6a6c582bf0accea75579dfbc",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyjuice-module-0.11.3-ubuntu-24.04-arm64.tar.gz",
            sha256: "a38b1bb52457380051a2a258210e780d85cdc6b3ed8f4bd947bb03dba5b9d401",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyjuice-module-0.11.3-ubuntu-24.04-x86_64.tar.gz",
            sha256: "5f4fb1b731ea2cd703e91df6d761e16d6a302a48fdd5eb4b37d3562ba4c93b4e",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyjuice-module-0.11.3-windows-11-arm64.zip",
            sha256: "42172fd462577264628ed78162f1c8cce82758ba9bc23162140df6695a2894bb",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyjuice-module-0.11.3-windows-2022-x86_64.zip",
            sha256: "41d07506848cc998b2a10d7b4d47412ef22ff831b7f3af201c7d67a2d37e17a2",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyjuice-module-0.11.3-windows-2025-x86_64.zip",
            sha256: "66ce9c539999f92bac927e1b4634264d075c627473c70a735882377cd2591b87",
        },
    ],
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinyjuice",
        manifest: "crates/tinyjuice-module/Cargo.toml",
        package: "tinyjuice-module",
        lib_name: "tinyjuice_module",
        features: &[],
    },
};
