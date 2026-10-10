//! Registry record for the `tinyjuice` module.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

pub(crate) const TINYJUICE: ModuleRecord = ModuleRecord {
    id: "tinyjuice",
    description: "Content-aware tool-output compression and recoverable caching",
    bus_name: "ai.tinyhumans.tinyjuice.Compression",
    object_path: "/ai/tinyhumans/tinyjuice/Compression",
    version: "0.7.0",
    release_url: "https://github.com/tinyhumansai/tinyjuice/releases/tag/v0.7.0",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyjuice-module-0.7.0-ubuntu-24.04-x86_64.tar.gz",
            sha256: "368e3173b226d4ef052e18e3fc94284d9d4c4e25be4c8f55215f5baa662ef641",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyjuice-module-0.7.0-ubuntu-24.04-arm64.tar.gz",
            sha256: "4a8ae99854c30c1141162f541a0ee7234ecca1fad14f80aafbb2387ab1766d3a",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyjuice-module-0.7.0-ubuntu-22.04-x86_64.tar.gz",
            sha256: "1ccebb91c91ecc6e57d0d5aaae26a0625f3262529191a19f2437cc082eac20b1",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyjuice-module-0.7.0-ubuntu-22.04-arm64.tar.gz",
            sha256: "b5a626c55f45edc02520dfed98c70bd4d273c676ad0853a54f641dcc3d0b2aef",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyjuice-module-0.7.0-macos-26-arm64.tar.gz",
            sha256: "994b02f2d04b8f3b58abd521fb4759256728f979d92487e031035d7580bd05d5",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyjuice-module-0.7.0-macos-26-x86_64.tar.gz",
            sha256: "3ac4b9c7813d83ad99859a3fb819644f21a4260751e394c4390542ac65bcf6d6",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyjuice-module-0.7.0-macos-15-arm64.tar.gz",
            sha256: "f93cf8af3e04603b5392ffe9f108b86fb55f9821339dc52196a3153ae7cf9a83",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyjuice-module-0.7.0-macos-15-x86_64.tar.gz",
            sha256: "cfb87a37aa0e7fa45f3b636ed20f0e58e8cd1b5da35be79b24c35b2bcb6fdf19",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyjuice-module-0.7.0-windows-2025-x86_64.zip",
            sha256: "60611a2f21cd4b3d6b1fb1f860b837656d1bd82ad1ed5d518467864be0056860",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyjuice-module-0.7.0-windows-2022-x86_64.zip",
            sha256: "0f18a10f9c71a3dcec440ec610a7972074bf6270b355d83173c625d03cc8f55b",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyjuice-module-0.7.0-windows-11-arm64.zip",
            sha256: "19393938ebe4661c1e25b68fd144420a3dc65658caa1cc311973900553f6aa25",
        },
    ],
    load: LoadPolicy::Lazy,
};
