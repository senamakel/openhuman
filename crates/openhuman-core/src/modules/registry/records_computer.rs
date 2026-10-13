//! Native computer-use module (desktop and browser control). Published digests are added only from its release manifest.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset, SourceSpec};
use tinycomputer_bus::names;

pub(crate) const TINYCOMPUTER: ModuleRecord = ModuleRecord {
    id: "tinycomputer",
    description: "Permission-aware desktop and browser observation and control",
    bus_name: names::INTERFACE,
    object_path: names::OBJECT_PATH,
    version: "0.12.0",
    release_url: "https://github.com/tinyhumansai/tinycomputer/releases/tag/v0.12.0",
    // All seventeen published archives were checked against checksum.toml
    // and GitHub asset digests before pinning. Linux permission probes are
    // part of the shipped voice surface alongside macOS and Windows.
    assets: &[
        PlatformAsset {
            host_key: "archlinux-arm64",
            archive: "tinycomputer-0.12.0-archlinux-arm64.tar.gz",
            sha256: "e8cc4af434de52d00596bc149d97af2fcde17a6df8a8020e673707f4c62e154d",
        },
        PlatformAsset {
            host_key: "archlinux-x86_64",
            archive: "tinycomputer-0.12.0-archlinux-x86_64.tar.gz",
            sha256: "0019e1b3566a42fc7689020bfbcb68d9ae6c1012b384968dc848ddc25e6db78a",
        },
        PlatformAsset {
            host_key: "fedora-43-arm64",
            archive: "tinycomputer-0.12.0-fedora-43-arm64.tar.gz",
            sha256: "32bef53dcb7fe5b6d4249872394dc3bc7bd9b492f1919fc3d10cac599ce44257",
        },
        PlatformAsset {
            host_key: "fedora-43-x86_64",
            archive: "tinycomputer-0.12.0-fedora-43-x86_64.tar.gz",
            sha256: "0b708f803f2a17b88527703e4e61896ace66836561cabe50630fe129865b2358",
        },
        PlatformAsset {
            host_key: "fedora-44-arm64",
            archive: "tinycomputer-0.12.0-fedora-44-arm64.tar.gz",
            sha256: "64a1fce22c35299c8287fd42527a6a7f597b33f194e02d0d5cf3c11c4d256e82",
        },
        PlatformAsset {
            host_key: "fedora-44-x86_64",
            archive: "tinycomputer-0.12.0-fedora-44-x86_64.tar.gz",
            sha256: "c36c5a48d18aed528df757dfb3528db62b68cd0c3d4742c89d310ac5357631e8",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinycomputer-0.12.0-macos-15-arm64.tar.gz",
            sha256: "f4c79740be1cf846d21465687d72bffc0933b69bef4461de472e8ecc3078cfc6",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinycomputer-0.12.0-macos-15-x86_64.tar.gz",
            sha256: "af5acd782c5512bf438d0d6b15a277958549d22c7b6c17e022c918eb74101007",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinycomputer-0.12.0-macos-26-arm64.tar.gz",
            sha256: "a1353832da53723b2fa8692cea211e3cb6bada080ed57c4b3fe4cddb820832fe",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinycomputer-0.12.0-macos-26-x86_64.tar.gz",
            sha256: "e7d0765ce556d3b8365dc97a72b86d3967d1df965499ab2963846a2f737cd9a5",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinycomputer-0.12.0-ubuntu-22.04-arm64.tar.gz",
            sha256: "c7afe5a49aac31e92533e6a1945641303147ba9d644ca4efdbf47614b134fd8d",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinycomputer-0.12.0-ubuntu-22.04-x86_64.tar.gz",
            sha256: "bdb730da52f42abc463fac4664f58c3c45185292150670ba5b8665d4870f549c",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinycomputer-0.12.0-ubuntu-24.04-arm64.tar.gz",
            sha256: "ec0c16a26ecb6a3fc4b3beee61158a3f1d6f0dd4709d1d07c53f1841393a8e30",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinycomputer-0.12.0-ubuntu-24.04-x86_64.tar.gz",
            sha256: "ab6f0cdc859dc16bbe8683666a801d25458e262951f6cdd0d1d859ef7dafdd2a",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinycomputer-0.12.0-windows-11-arm64.zip",
            sha256: "6f38f0e26466ec7d325a954a8d85ab6b9b369b4994674813913d57d32c7c0e44",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinycomputer-0.12.0-windows-2022-x86_64.zip",
            sha256: "511474fe1e2791f522830c230241d900e10ffcfcb673e2327e629a7a8ed02218",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinycomputer-0.12.0-windows-2025-x86_64.zip",
            sha256: "132987857df720f6d6cea45e4b1b5c5107852154248bd132088896335a859a96",
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
