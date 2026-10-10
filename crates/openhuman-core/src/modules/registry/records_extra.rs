//! Registry records for additional first-party TinyBus modules.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

/// The `tinybox` module, loaded on demand.
pub(crate) const TINYBOX: ModuleRecord = ModuleRecord {
    id: "tinybox",
    description: "Sandbox capability discovery through TinyBox",
    bus_name: "ai.tinyhumans.tinybox.Box",
    object_path: "/ai/tinyhumans/tinybox/Box",
    version: "0.1.16",
    release_url: "https://github.com/tinyhumansai/tinybox/releases/tag/v0.1.16",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinybox-0.1.16-ubuntu-24.04-x86_64.tar.gz",
            sha256: "3163f7cf621beaf71d99b978555085e6bb933a0810153970dd16b2c531e1a030",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinybox-0.1.16-ubuntu-24.04-arm64.tar.gz",
            sha256: "fad323bf74ce075f758a31c7cb93f393d84ac4c9a6a22f31fb7fc7e2ec4743c4",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinybox-0.1.16-ubuntu-22.04-x86_64.tar.gz",
            sha256: "9abddc0e8ad14ac9f34e479714baa7f1720ed029d199272214450f356b7d51da",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinybox-0.1.16-ubuntu-22.04-arm64.tar.gz",
            sha256: "fb164eeec76789035de8c621176630b6ea6aea0a5021778fbd97fafe163b6dac",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinybox-0.1.16-macos-26-arm64.tar.gz",
            sha256: "34cb87457a3b21ec5ee8b8b6817f6a78a854b12e4d40fa43221aeed508d7c40a",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinybox-0.1.16-macos-26-x86_64.tar.gz",
            sha256: "5f7a4886fb191a58dc33bb5f43a7034c0f6b31c1e85161e13228aa0deb6ae5b9",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinybox-0.1.16-macos-15-arm64.tar.gz",
            sha256: "ae427b7962b0607051595c9c12f9cb630c87672bc3c92c7becdac87283aaefd4",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinybox-0.1.16-macos-15-x86_64.tar.gz",
            sha256: "417d4c182c3882cd90b5ff323dc81c62c2d98b3e0bb8ac0a237cac51c34828fe",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinybox-0.1.16-windows-2025-x86_64.zip",
            sha256: "ada7ddb1edf16887ef20d84241f0453b2ce8ee29aa670c1505758c1f7913b4fc",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinybox-0.1.16-windows-2022-x86_64.zip",
            sha256: "044666f53b10c8db6626262de45806d1a2a34147ce2de196e8549987f32c1cd8",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinybox-0.1.16-windows-11-arm64.zip",
            sha256: "bc3da524fc3e702e77cc1e85c064dccaece633f896503737c71ce7127ba26440",
        },
    ],
    load: LoadPolicy::Lazy,
};

/// The `tinychannels` module, loaded on demand.
pub(crate) const TINYCHANNELS: ModuleRecord = ModuleRecord {
    id: "tinychannels",
    description: "Channel provider lifecycle and message transport",
    bus_name: "ai.tinyhumans.tinychannels.Channels",
    object_path: "/ai/tinyhumans/tinychannels/Channels",
    version: "0.1.12",
    release_url: "https://github.com/tinyhumansai/tinychannels/releases/tag/v0.1.12",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinychannels-module-0.1.12-ubuntu-24.04-x86_64.tar.gz",
            sha256: "4d238ccecc5e2ac40006e17dfe51eeae3e9d5504395db852d83037f72aaa296f",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinychannels-module-0.1.12-ubuntu-24.04-arm64.tar.gz",
            sha256: "9474764c23ed7e3e83ad28611d0e2d403441ee0c8d6f2f643b0e7c190b0065d1",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinychannels-module-0.1.12-ubuntu-22.04-x86_64.tar.gz",
            sha256: "deb8313e1c5607b83d314b61c2b6305f104a6c0dd5ce19f7ef65018d4d3f78b0",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinychannels-module-0.1.12-ubuntu-22.04-arm64.tar.gz",
            sha256: "3f1a85cce6e19959f9ba29829f82c8bd195f553d4fd2b11d9f4de9fdf7243604",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinychannels-module-0.1.12-macos-26-arm64.tar.gz",
            sha256: "64131eb839e356cb1e3519cd0b10d6ab355ff31330f4b4531523567e3de7c577",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinychannels-module-0.1.12-macos-26-x86_64.tar.gz",
            sha256: "c92985145ca92b52a4af2ae7e2a881d4f4fa2aa78bd285bede4399459a8a4153",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinychannels-module-0.1.12-macos-15-arm64.tar.gz",
            sha256: "bdd48673381b80997a6916d85912fa6fbd6aae477b77ff296524bf9297b6abd8",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinychannels-module-0.1.12-macos-15-x86_64.tar.gz",
            sha256: "8c595292d9ac46d2af0c81c22a2856b0f7fc22f9dcc8bbd567bbc410b5073954",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinychannels-module-0.1.12-windows-2025-x86_64.zip",
            sha256: "4ccddd11a8057e18a55a924bf46d4670c3a34ba0e1a7fccd95e848504d24d3cf",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinychannels-module-0.1.12-windows-2022-x86_64.zip",
            sha256: "e856df1a46c8e966325075320e37d1443671058c23c46ee517c65b6f42912abf",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinychannels-module-0.1.12-windows-11-arm64.zip",
            sha256: "c9c5167fd8c19581f8b15d68cc2871627ac3006573d0ee40e709ca16bb21f109",
        },
    ],
    load: LoadPolicy::Lazy,
};

/// The `tinyhosts` module, loaded on demand.
pub(crate) const TINYHOSTS: ModuleRecord = ModuleRecord {
    id: "tinyhosts",
    description: "Hosting provider operations",
    bus_name: "ai.tinyhumans.tinyhosts.Hosting",
    object_path: "/ai/tinyhumans/tinyhosts/Hosting",
    version: "0.2.3",
    release_url: "https://github.com/tinyhumansai/tinyhosts/releases/tag/v0.2.3",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyhosts-0.2.3-ubuntu-24.04-x86_64.tar.gz",
            sha256: "a0e623c5f541c1d21f2e7aba16ce3074fcee06f014c39c43fd2a7f7a3a77f6dd",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyhosts-0.2.3-ubuntu-24.04-arm64.tar.gz",
            sha256: "0e0b0467c08e3f79275fbbd46e84df3dea570f896b746031a581d030eb7eb9c5",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyhosts-0.2.3-ubuntu-22.04-x86_64.tar.gz",
            sha256: "6d622d9497ebdea67323125e25dab0d6497e0d3f39c351f3ef13de016076fc8b",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyhosts-0.2.3-ubuntu-22.04-arm64.tar.gz",
            sha256: "6648fa3debb518ab5090aef2e49a3eed147ca00eef5c0c7e22576768cb7d1f51",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyhosts-0.2.3-macos-26-arm64.tar.gz",
            sha256: "8c8d62564df5fad2b3d4da30e56e86969584aacb30a22875f96a6209d1e0cfae",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyhosts-0.2.3-macos-26-x86_64.tar.gz",
            sha256: "801cec78dfe01006ff6811b445730de3f246087f1cbe9f0d63da8bcf55bf3f5d",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyhosts-0.2.3-macos-15-arm64.tar.gz",
            sha256: "10b9ac5b6e414064b4d9abb9cc4cb47b7a6e80e0cbfd0c29a8b1afe41637fb0f",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyhosts-0.2.3-macos-15-x86_64.tar.gz",
            sha256: "3b9b608d4fa3b638c93485fefca3825ebdc4220d0fc4dc2312f94a570dadcb83",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyhosts-0.2.3-windows-2025-x86_64.zip",
            sha256: "b9caaed8b6b2235098c88477d8101f2dcd26e4d5a68109957836272616f25b71",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyhosts-0.2.3-windows-2022-x86_64.zip",
            sha256: "a1ca38023401efd5fda4db6f8b30701dd9a209563f74d5a961e6572e7c4b6223",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyhosts-0.2.3-windows-11-arm64.zip",
            sha256: "3912ad5a10ce87e2de5e91e29f8beb1ffb669e31ca2dcdd7e0e522b63607adf7",
        },
    ],
    load: LoadPolicy::Lazy,
};
