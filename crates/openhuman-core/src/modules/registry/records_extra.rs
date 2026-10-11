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
    version: "0.1.13",
    release_url: "https://github.com/tinyhumansai/tinychannels/releases/tag/v0.1.13",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinychannels-module-0.1.13-ubuntu-24.04-x86_64.tar.gz",
            sha256: "3fe360f6863a01e2fe48737d9895fd997467121ee1fa9bb30e7f875368c9b5c9",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinychannels-module-0.1.13-ubuntu-24.04-arm64.tar.gz",
            sha256: "e636f26b9484b351409f1c1ac0f22ee61435a24961cb0f57e2b7537774b0934a",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinychannels-module-0.1.13-ubuntu-22.04-x86_64.tar.gz",
            sha256: "f0ca5091e42c1493311db34015d4485a557700100feac4245468c3d0f64eef08",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinychannels-module-0.1.13-ubuntu-22.04-arm64.tar.gz",
            sha256: "2824102014b345c21c9d7e16fae8917cdf6fea03529a67de1ee58065f1a694e6",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinychannels-module-0.1.13-macos-26-arm64.tar.gz",
            sha256: "81e16aa479839a673b03a18bd715ebc1b5efef7e6f223953233688ad08d1bbf2",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinychannels-module-0.1.13-macos-26-x86_64.tar.gz",
            sha256: "dc82f10309946132e7837fb97c86203d79741416536b0d944972cb8acaa3f220",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinychannels-module-0.1.13-macos-15-arm64.tar.gz",
            sha256: "7770a906ac3bb46e89725363d723e32709b45300d234b360c4822666c29f7c51",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinychannels-module-0.1.13-macos-15-x86_64.tar.gz",
            sha256: "cbd540871efc81822181ddd51502c93f31e2a99a8be58b725d01bc70d7d9ba6d",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinychannels-module-0.1.13-windows-2025-x86_64.zip",
            sha256: "a7fee0a4bcbb52b4f7866eab6a1c4eca31fa7de8541c8321c61314c1286f981a",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinychannels-module-0.1.13-windows-2022-x86_64.zip",
            sha256: "1a74d967ac4f859d6c210865788fcdb124accc027e75823eb3d9a55a700a0dcd",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinychannels-module-0.1.13-windows-11-arm64.zip",
            sha256: "cbcaaeed4c6e2c3baae1474686e210281c16d8f75024d1a4805f68cbbc8f1f67",
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
    version: "0.3.0",
    release_url: "https://github.com/tinyhumansai/tinyhosts/releases/tag/v0.3.0",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyhosts-0.3.0-ubuntu-24.04-x86_64.tar.gz",
            sha256: "368f00ee397649481ff8f61d33eeb8b908cc416d6b2f9d4ef3c81263d6b0460e",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyhosts-0.3.0-ubuntu-24.04-arm64.tar.gz",
            sha256: "e298942fddec8930fd78651632c02abe78968a17ab3589608875b8dc4f731669",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyhosts-0.3.0-ubuntu-22.04-x86_64.tar.gz",
            sha256: "f7c9484176b62357fb449eb79830e3df0eebfd4e0814c8d8e1c58669d829f482",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyhosts-0.3.0-ubuntu-22.04-arm64.tar.gz",
            sha256: "576ff6204ccaa1e2dc2ef1b27762848efa4555d4d32ef46b4c329daf1e04eac3",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyhosts-0.3.0-macos-26-arm64.tar.gz",
            sha256: "b0083e70b567659d095a1ff0f9d2c8cb5bafe110d90fb08527c509362ce77a55",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyhosts-0.3.0-macos-26-x86_64.tar.gz",
            sha256: "44fa7c9efde0c388f154f154809ce2829460bae4931f27d20f8f0d3ee3bf2a8c",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyhosts-0.3.0-macos-15-arm64.tar.gz",
            sha256: "56ca93b2cab2010ff599c38179eb99f6ad67631e39f671be973256c3d9142381",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyhosts-0.3.0-macos-15-x86_64.tar.gz",
            sha256: "0ffb9fc8d1c7fc7d95a7808af7722d8c6537a508c0c3a294ecf8fb8a28430207",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyhosts-0.3.0-windows-2025-x86_64.zip",
            sha256: "3d6d5f7fd0806ac6fd6c2ed0c60a9155d6c09a861ebd1ac0c43864e969f88fb9",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyhosts-0.3.0-windows-2022-x86_64.zip",
            sha256: "a5003f1254052828d6fa362b993b0bed6b7256c252f969536090e148378c331f",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyhosts-0.3.0-windows-11-arm64.zip",
            sha256: "21f8909a2a81a541e3d0347510b2bf25baec59438a5536edcd57e65fae7f4a4d",
        },
    ],
    load: LoadPolicy::Lazy,
};
