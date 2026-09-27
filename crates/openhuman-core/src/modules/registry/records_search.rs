//! TinySearch web-search module. Published digests are added only from its
//! release manifest.
use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

pub(crate) const TINYSEARCH: ModuleRecord = ModuleRecord {
    id: "tinysearch",
    description:
        "Web search, grounded answers and page contents across providers through TinySearch",
    bus_name: tinysearch_bus::names::INTERFACE,
    object_path: tinysearch_bus::names::OBJECT_PATH,
    version: "0.3.0",
    release_url: "https://github.com/tinyhumansai/tinysearch/releases/tag/v0.3.0",
    // Verbatim from the published v0.3.0 checksum.toml; the same host set as
    // the other native modules (macOS, Ubuntu, Windows).
    assets: &[
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinysearch-0.3.0-macos-26-arm64.tar.gz",
            sha256: "8808a3713bcd11643fa8ea927fc132036a58dbebdae101fcabc7e6a7da9ae577",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinysearch-0.3.0-macos-26-x86_64.tar.gz",
            sha256: "fe7cccd3cbf2895e15e2d9b734c36451396fef66accd94f092f922488c4b6409",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinysearch-0.3.0-macos-15-arm64.tar.gz",
            sha256: "5bdb8084da5d8358c17363ad7f8de67bbfbdd52707a93c4daa5eb36f8a8e5649",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinysearch-0.3.0-macos-15-x86_64.tar.gz",
            sha256: "03d9b2ede4f3dc8f1b8b95c7cca1aad53486a9a6217aa119a5cc014efbf165e5",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinysearch-0.3.0-ubuntu-24.04-x86_64.tar.gz",
            sha256: "3a81f755eb113bdbaa90f1d98ead49b8d807d1ee5bbf26de9ebd9a8fa6200d79",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinysearch-0.3.0-ubuntu-24.04-arm64.tar.gz",
            sha256: "4f7032bc7e6ac3e25039fc7ba29d55fd592bad2f1ad1d3fb97dc67fe377f1cf1",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinysearch-0.3.0-ubuntu-22.04-x86_64.tar.gz",
            sha256: "b1767a1046a4422f5c1cb80af245f088caa3874898c7bc34c1ce060f361df419",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinysearch-0.3.0-ubuntu-22.04-arm64.tar.gz",
            sha256: "9d3b22a4bab42dc8ce8fa3dbff01a96443233da1cc4a31cec26fb2b75a26a1a6",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinysearch-0.3.0-windows-2025-x86_64.zip",
            sha256: "a7deb9d5695d04e48aa21bea7215b0f84c5a307cfd4019e972398b4e4a4d4b5a",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinysearch-0.3.0-windows-2022-x86_64.zip",
            sha256: "0340408df29f5eabd62037ae709fa569815aa6acdbeff1df51bce042b6c2a07f",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinysearch-0.3.0-windows-11-arm64.zip",
            sha256: "a46672aa4b1a96630345aaf481d83deef479c5670bf628055cb49a0137ed19e0",
        },
    ],
    load: LoadPolicy::Lazy,
};
