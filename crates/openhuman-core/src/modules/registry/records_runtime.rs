//! Registry records for the `tinyruntime` router and its Node.js and Python
//! sidecars.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

/// The `tinyruntime` module: the runtime router.
///
/// Resolves a language runtime, installs one when the host has none, reuses one
/// when it does, and runs code on a bounded pool of warm interpreter processes.
/// It is a router: on its own it knows no languages, and it routes to the two
/// provider records below.
///
/// Lazy, because a host that never runs a skill, a flow step, or a `node_exec`
/// should not pay a download and a `dlopen` for the ability to.
///
/// The digests below are v0.2.2's, taken verbatim from that release's
/// `checksum.toml`. Until it existed this record carried no assets at all and
/// the module was reachable only from a developer build named by
/// `modules.local` or found on `OPENHUMAN_MODULE_PATH` — so on any machine that
/// had not built it, the runtime domain was a set of tools that could not run.
pub(crate) const TINYRUNTIME: ModuleRecord = ModuleRecord {
    id: "tinyruntime",
    description: "Language runtime resolution, installation, and pooled execution",
    bus_name: "ai.tinyhumans.runtime.Runtime",
    object_path: "/ai/tinyhumans/runtime/Runtime",
    version: "0.2.6",
    release_url: "https://github.com/tinyhumansai/tinyruntime/releases/tag/v0.2.6",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyruntime-0.2.6-ubuntu-24.04-x86_64.tar.gz",
            sha256: "81e24f7695cbeb8580085c8dbc0c948cc19b4635cd11e42e3b42aaa2c6087a21",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyruntime-0.2.6-ubuntu-24.04-arm64.tar.gz",
            sha256: "3758f492a1c71e9567411e1439868dd67a96293dfc30fc8607251b1d9e509841",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyruntime-0.2.6-ubuntu-22.04-x86_64.tar.gz",
            sha256: "122fc28b75bf77faa38b18380f471994cd8df89a6b7329be85548b5e6e8b2871",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyruntime-0.2.6-ubuntu-22.04-arm64.tar.gz",
            sha256: "3ea99bd5f7095e6b4944aacf9cf305b59eb4f5e8735158c3075b0bf56446f5f4",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyruntime-0.2.6-macos-26-arm64.tar.gz",
            sha256: "3a49b9d48bb75d44c297609caf8acf92b28ca2ad70ace376928be9399aff5030",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyruntime-0.2.6-macos-26-x86_64.tar.gz",
            sha256: "ed7ae6bd460f657b61a0f688d8a5e4c395491a005b22c8774daa8c59e35b6c3f",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyruntime-0.2.6-macos-15-arm64.tar.gz",
            sha256: "a713b298a01af864c68f70bd46c1c909cb37034aaba4ac07a3d00afa5fd09dd7",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyruntime-0.2.6-macos-15-x86_64.tar.gz",
            sha256: "388c5793467860b121a915febeefdbe5112ffa7ac58e6794b20e0d9b9872e542",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyruntime-0.2.6-windows-2025-x86_64.zip",
            sha256: "6b2ab049421e180f43406273eb8f1bf5efaf7797fba7aaf48630f91102559298",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyruntime-0.2.6-windows-2022-x86_64.zip",
            sha256: "44396066f25dc18d2ebd6ec17a6898d1c110fa5019d2bba5e8c8866e4e952b6b",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyruntime-0.2.6-windows-11-arm64.zip",
            sha256: "a86eba2bb4c04658097d1e681d8ef3b2fb793973d5ab10eae9e35ca45d943a0e",
        },
    ],
    load: LoadPolicy::Lazy,
};

/// The `tinyruntime-nodejs` module: the Node.js half of the router's knowledge.
///
/// Answers which host interpreters count, which archive nodejs.org publishes for
/// this machine, where the binaries land, and what a warm Node worker is. It
/// installs nothing itself.
///
/// It implements the shared `ai.tinyhumans.runtime.Provider` interface but
/// serves at its own object path, because two modules cannot claim one bus name
/// and tinybus derives the path from the name.
///
/// Lazy, and loaded by the same call that loads the router: a language is only
/// worth its `dlopen` when something asks for that language.
///
/// Released alongside the router and pinned the same way — see [`TINYRUNTIME`].
pub(crate) const TINYRUNTIME_NODEJS: ModuleRecord = ModuleRecord {
    id: "tinyruntime-nodejs",
    description: "Node.js runtime provider for tinyruntime",
    bus_name: "ai.tinyhumans.runtime.nodejs.Provider",
    object_path: "/ai/tinyhumans/runtime/nodejs/Provider",
    version: "0.2.3",
    release_url: "https://github.com/tinyhumansai/tinyruntime-nodejs/releases/tag/v0.2.3",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyruntime-nodejs-0.2.3-ubuntu-24.04-x86_64.tar.gz",
            sha256: "b23f24d50059a4eae1879bedfa5ca96855fe7001b3054e46bb19a1366e9d457d",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyruntime-nodejs-0.2.3-ubuntu-24.04-arm64.tar.gz",
            sha256: "65cf1212cf9c1038f5e5640bb966121375f168f5768dda006469a586ddae2347",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyruntime-nodejs-0.2.3-ubuntu-22.04-x86_64.tar.gz",
            sha256: "27926aa4368ffa742fa771c3b764005dcc4e0da69b3a60be407185b417325753",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyruntime-nodejs-0.2.3-ubuntu-22.04-arm64.tar.gz",
            sha256: "c0f4f7a9976594282cf6cf57d9aedddb48b41efa719661e337e53be9a4ac8b36",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyruntime-nodejs-0.2.3-macos-26-arm64.tar.gz",
            sha256: "c4fd45df9cfd3b31eae2ceb32eae8b34ab6074a8cd79c3928d554ed9c5270ae6",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyruntime-nodejs-0.2.3-macos-26-x86_64.tar.gz",
            sha256: "b21b95c5ff0ced899586bf412e5b7c742dc6b071ceca55cd44194e8f71fa9a6c",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyruntime-nodejs-0.2.3-macos-15-arm64.tar.gz",
            sha256: "978ce423827ac5a36be0976efa9ac0c372b62431414fb129e4583f8d63556d4d",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyruntime-nodejs-0.2.3-macos-15-x86_64.tar.gz",
            sha256: "dfeed3e3d4fc32a917a4ee128ae676ed6a0f6f2dcbbbe8d31c43c7596e9d7c17",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyruntime-nodejs-0.2.3-windows-2025-x86_64.zip",
            sha256: "4fa99aee9d04857c93c503c7049c6d45ef637473f4151cc43365b496bba06354",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyruntime-nodejs-0.2.3-windows-2022-x86_64.zip",
            sha256: "874072c77cbc4d257ade8a9619ded7c5b70d164a40cf5d8f3e8365eafdcb7085",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyruntime-nodejs-0.2.3-windows-11-arm64.zip",
            sha256: "1f72cf3f98ab35438b87c15e0ab66df11d1c4fafd412fe56957630c8904eb2f4",
        },
    ],
    load: LoadPolicy::Lazy,
};

/// The `tinyruntime-python` module: the Python half of the router's knowledge.
///
/// Answers which host interpreters count, which standalone build to install, and
/// what a warm Python worker is. It installs nothing itself.
///
/// Released alongside the router and pinned the same way — see [`TINYRUNTIME`].
pub(crate) const TINYRUNTIME_PYTHON: ModuleRecord = ModuleRecord {
    id: "tinyruntime-python",
    description: "Python runtime provider for tinyruntime",
    bus_name: "ai.tinyhumans.runtime.python.Provider",
    object_path: "/ai/tinyhumans/runtime/python/Provider",
    version: "0.2.3",
    release_url: "https://github.com/tinyhumansai/tinyruntime-python/releases/tag/v0.2.3",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyruntime-python-0.2.3-ubuntu-24.04-x86_64.tar.gz",
            sha256: "884e166bbd47ba8ba8ace330204a5257732acb4afa0efa978589d6f1310413e2",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyruntime-python-0.2.3-ubuntu-24.04-arm64.tar.gz",
            sha256: "c2cccb14258b9bf0bc5f4d004a4b6f977e74aef238c20b28a583bbb2c8f46902",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyruntime-python-0.2.3-ubuntu-22.04-x86_64.tar.gz",
            sha256: "1d7e4ac2ecd1540e1483d6cfc8de21d8047bd1c25cfbbc2b4e7d1ef282a901f7",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyruntime-python-0.2.3-ubuntu-22.04-arm64.tar.gz",
            sha256: "6bfeb25be2fb905e99f95d4b22eb75e0ecf33daa25d0ecaf7659df867cab1351",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyruntime-python-0.2.3-macos-26-arm64.tar.gz",
            sha256: "029e23c57e7b1d7655de71fd1a9f9d07034f8ee3ad2bc9cc9756bbf3eb2e1d00",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyruntime-python-0.2.3-macos-26-x86_64.tar.gz",
            sha256: "c2bb8c490948ef0704970a13f4ea9a251c1268012516bf1065a3f43c49836542",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyruntime-python-0.2.3-macos-15-arm64.tar.gz",
            sha256: "5adfd8e4b5ec4cc0078830b407216deaad59202e505281295304b28b01ce8bac",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyruntime-python-0.2.3-macos-15-x86_64.tar.gz",
            sha256: "1aa380725ef7acac0117a5b82baa50521a0a20f88463eb8af863327cc0c7182d",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyruntime-python-0.2.3-windows-2025-x86_64.zip",
            sha256: "6191a1f2250071ffa0c8b8c199c1ef06dba22250e371ffc66880107b6643cdf6",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyruntime-python-0.2.3-windows-2022-x86_64.zip",
            sha256: "90054b3a0c1663ec02de4bac4a47778ed127338a61c6bbdbdd4653e7e18b1c23",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyruntime-python-0.2.3-windows-11-arm64.zip",
            sha256: "a319e513431897a94faa0844a00451a35134bd302ce1b7f80637e002df0c15a0",
        },
    ],
    load: LoadPolicy::Lazy,
};
