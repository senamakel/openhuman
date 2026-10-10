//! TinySearch web-search module. Published digests are added only from its
//! release manifest.
use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

pub(crate) const TINYSEARCH: ModuleRecord = ModuleRecord {
    id: "tinysearch",
    description:
        "Web search, grounded answers and page contents across providers through TinySearch",
    bus_name: tinysearch_bus::names::INTERFACE,
    object_path: tinysearch_bus::names::OBJECT_PATH,
    version: "0.4.0",
    release_url: "https://github.com/tinyhumansai/tinysearch/releases/tag/v0.4.0",
    // Verbatim from the published v0.4.0 checksum.toml; the same host set as
    // the other native modules (macOS, Ubuntu, Windows).
    assets: &[
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinysearch-0.4.0-macos-26-arm64.tar.gz",
            sha256: "63cd720117b6751a12a45049e77d4891abb2282174b3af7ebc6637d7801a000c",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinysearch-0.4.0-macos-26-x86_64.tar.gz",
            sha256: "6e5727704b2ce42907c93ef7610bb6a709700a6d9c52aeffda6dc287760f8733",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinysearch-0.4.0-macos-15-arm64.tar.gz",
            sha256: "0d3d67648779476359bc7f5c601df6f18a1e25ee99d05076d3851fecbec271d2",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinysearch-0.4.0-macos-15-x86_64.tar.gz",
            sha256: "dd0884894a05046377d717778d78c9523a3fbafe42ea09385963cc7fdec94205",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinysearch-0.4.0-ubuntu-24.04-x86_64.tar.gz",
            sha256: "12f3b5d9377d6411a2acd027694a587d301f1823bdf33920e422494e9d7b46d6",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinysearch-0.4.0-ubuntu-24.04-arm64.tar.gz",
            sha256: "0e90ab3abd25ca9a622f1616facc10cee1ecb6aeab0f4be9ac8fd0d53f8bdd87",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinysearch-0.4.0-ubuntu-22.04-x86_64.tar.gz",
            sha256: "bc84f7f509a26de55b5491348f47e44a41a48755976e86172f50f1c5252bd124",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinysearch-0.4.0-ubuntu-22.04-arm64.tar.gz",
            sha256: "87e3151bbcd8c4c4eebe16ab533534f93c186e15abba7e050de8fdeb6690046d",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinysearch-0.4.0-windows-2025-x86_64.zip",
            sha256: "1701290dd580be7c7c3ea9d75fd4bf4f39dfbbb98dba0cdc90875a6553a02284",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinysearch-0.4.0-windows-2022-x86_64.zip",
            sha256: "82e2c12f3820cf7ac61d721f109defa9fcc9a8553ac48a591e8fbafeb9f8a49f",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinysearch-0.4.0-windows-11-arm64.zip",
            sha256: "be1084a2b820c8c127106bb4c9a9c47df9b0fc27f68107e2e06e46f137fd562b",
        },
    ],
    load: LoadPolicy::Lazy,
};
