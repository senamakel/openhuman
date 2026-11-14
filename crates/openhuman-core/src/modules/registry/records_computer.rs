//! Native computer-use module (desktop and browser control). Published digests are added only from its release manifest.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset, SourceSpec};
use tinycomputer_bus::names;

pub(crate) const TINYCOMPUTER: ModuleRecord = ModuleRecord {
    id: "tinycomputer",
    description: "Permission-aware desktop and browser observation and control",
    bus_name: names::INTERFACE,
    object_path: names::OBJECT_PATH,
    version: "0.16.1",
    release_url: "https://github.com/tinyhumansai/tinycomputer/releases/tag/v0.16.1",
    // All seventeen published archives were checked against checksum.toml
    // and GitHub asset digests before pinning. Linux permission probes are
    // part of the shipped voice surface alongside macOS and Windows.
    assets: &[
        PlatformAsset {
            host_key: "archlinux-arm64",
            archive: "tinycomputer-0.16.1-archlinux-arm64.tar.gz",
            sha256: "61e31f69269370222513e814a8214c206a6f6e1eecc082ad1849a3ea06332b36",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "archlinux-x86_64",
            archive: "tinycomputer-0.16.1-archlinux-x86_64.tar.gz",
            sha256: "3f16a02e1b514bbc1cd253194c255e31434887914234e25905b71da56c0fa32a",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "fedora-43-arm64",
            archive: "tinycomputer-0.16.1-fedora-43-arm64.tar.gz",
            sha256: "17e05040028c54b24635e3b15a40e8d8b991e2e761adfe4653d520334dd2b563",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "fedora-43-x86_64",
            archive: "tinycomputer-0.16.1-fedora-43-x86_64.tar.gz",
            sha256: "ff83310c6d096a3fc242daa22e5de793e2ebf3aededb23c19118c983653bdc0d",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "fedora-44-arm64",
            archive: "tinycomputer-0.16.1-fedora-44-arm64.tar.gz",
            sha256: "f23ae2c2f707238a4da065753774c09fa086884dd5aaa4457c2dfd63cfed8aef",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "fedora-44-x86_64",
            archive: "tinycomputer-0.16.1-fedora-44-x86_64.tar.gz",
            sha256: "efeabbfab0752d3d298c7de100e5e3e04e9aef20edf77fa5f9af39325e02223e",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinycomputer-0.16.1-macos-15-arm64.tar.gz",
            sha256: "b2c65bb60e8920aa635c008a12a79b1c5dbc31095f4692a12ac7abe9015658ff",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinycomputer-0.16.1-macos-15-x86_64.tar.gz",
            sha256: "7dde8666a3c434f0c026e1cf2bcbeb638ad32350487c265e5860277b894c6426",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinycomputer-0.16.1-macos-26-arm64.tar.gz",
            sha256: "64e73fab406cd9c2171186e12b96a73b2ec5ad0e6a128909018461160ce65a0e",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinycomputer-0.16.1-macos-26-x86_64.tar.gz",
            sha256: "c308d3db1c9be71117ea28ff9d28cebfb41e9fa75f6be8c4c7bb0cfe17f3aeed",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinycomputer-0.16.1-ubuntu-22.04-arm64.tar.gz",
            sha256: "43c40bff8cf7a7f5a058a96f28895a628d54bc1a9d62e9a354a0e86f52eff6a0",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinycomputer-0.16.1-ubuntu-22.04-x86_64.tar.gz",
            sha256: "625c3bd95d9f170e8794d3bf54e1dc4c2e2356c921e8277c65e45efe92fa1cc6",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinycomputer-0.16.1-ubuntu-24.04-arm64.tar.gz",
            sha256: "67a7d741cc17c3e59b4021ce78505b9f99b93ca92b8b247bcf650c152c5e7652",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinycomputer-0.16.1-ubuntu-24.04-x86_64.tar.gz",
            sha256: "d2394934e358040dd27869da3527d79fca45bfa14721dc7c0d4b345ee3c6a78b",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinycomputer-0.16.1-windows-11-arm64.zip",
            sha256: "1d48f1efddf3b32ced18e5fe8d09e3d564dedca8f8ee89c20a2c8ffdb08f2035",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinycomputer-0.16.1-windows-2022-x86_64.zip",
            sha256: "819c7d3911e97e615336add99a8504ad47dc40f26cce33085d7c43b1f985a4f2",
            library_sha256: None,
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinycomputer-0.16.1-windows-2025-x86_64.zip",
            sha256: "9d970bfcefacac52603d7ee3fe3fc8e00bf962ed8be291ebb9c66b7d0696f8f4",
            library_sha256: None,
        },
    ],
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinycomputer",
        manifest: "crates/tinycomputer/Cargo.toml",
        package: "tinycomputer",
        lib_name: "tinycomputer",
        features: &[],
    },
};
