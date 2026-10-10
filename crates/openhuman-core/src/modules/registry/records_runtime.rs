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
/// The digests below are v0.2.12's, taken verbatim from that release's
/// `checksum.toml`. Until it existed this record carried no assets at all and
/// the module was reachable only from a developer build named by
/// `modules.local` or found on `OPENHUMAN_MODULE_PATH` — so on any machine that
/// had not built it, the runtime domain was a set of tools that could not run.
pub(crate) const TINYRUNTIME: ModuleRecord = ModuleRecord {
    id: "tinyruntime",
    description: "Language runtime resolution, installation, and pooled execution",
    bus_name: "ai.tinyhumans.runtime.Runtime",
    object_path: "/ai/tinyhumans/runtime/Runtime",
    version: "0.2.12",
    release_url: "https://github.com/tinyhumansai/tinyruntime/releases/tag/v0.2.12",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyruntime-0.2.12-ubuntu-24.04-x86_64.tar.gz",
            sha256: "5df89d2e8f10602e392e1d2fa0d8d463822a1db1362b9efc19c76ae8fd35da15",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyruntime-0.2.12-ubuntu-24.04-arm64.tar.gz",
            sha256: "c631f2825a7b1fe3e2395581022e8f134b01aa48a7ef1d3b5f39226107651a5a",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyruntime-0.2.12-ubuntu-22.04-x86_64.tar.gz",
            sha256: "c8434e10cb0fec5e04dc52a4245fd0bbce31a33fc45ecc52931b610873000347",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyruntime-0.2.12-ubuntu-22.04-arm64.tar.gz",
            sha256: "51423639e15d88872fb30cfc47bf3ea6a7820be701bba4d74c8d5989baeff451",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyruntime-0.2.12-macos-26-arm64.tar.gz",
            sha256: "efa190b4ae736fa53b6df8afc77e94f0dff187d17cf98c8e3bfa0b1f935dac38",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyruntime-0.2.12-macos-26-x86_64.tar.gz",
            sha256: "dab8eb686c22a2af8607b0d92e4d8eafdc6badc50cce39a97c56738dd9d8b2b6",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyruntime-0.2.12-macos-15-arm64.tar.gz",
            sha256: "e656ea5a6ba73d548339df13815ee869259b94762d3963e824431578779b0023",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyruntime-0.2.12-macos-15-x86_64.tar.gz",
            sha256: "96f6f31afe3896a333d33c650c9bbf89e615af01c0143b81c85dc911b393be43",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyruntime-0.2.12-windows-2025-x86_64.zip",
            sha256: "87da4cb46edd0acdcea9795d0b357087032924661d459362704dafb612a7a5d4",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyruntime-0.2.12-windows-2022-x86_64.zip",
            sha256: "8779766c73bb3050094d226c4019df124b7516a35fb3dc7eaee2d2011d9ec29b",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyruntime-0.2.12-windows-11-arm64.zip",
            sha256: "977e5632f98383348b309fa6ee3cc6069f5b2b9dc90585b1c43b5ce0d120bf17",
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
/// Released from its own repository (own version line) against the router's source pin; see scripts/ci/module-provider-pins.json.
pub(crate) const TINYRUNTIME_NODEJS: ModuleRecord = ModuleRecord {
    id: "tinyruntime-nodejs",
    description: "Node.js runtime provider for tinyruntime",
    bus_name: "ai.tinyhumans.runtime.nodejs.Provider",
    object_path: "/ai/tinyhumans/runtime/nodejs/Provider",
    version: "0.2.7",
    release_url: "https://github.com/tinyhumansai/tinyruntime-nodejs/releases/tag/v0.2.7",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyruntime-nodejs-0.2.7-ubuntu-24.04-x86_64.tar.gz",
            sha256: "bea90f1ed91793a3e06f3e7b92c751292cc00ffdbf7e6886fc4fa2334e6ef97f",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyruntime-nodejs-0.2.7-ubuntu-24.04-arm64.tar.gz",
            sha256: "8f3f99a9678c784ae8568a6991c2bf6ab0df90b571565299a674a53f489998e2",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyruntime-nodejs-0.2.7-ubuntu-22.04-x86_64.tar.gz",
            sha256: "c6eebb7bb20dbaca76de4628e442ca112fdab22e5b0d71eeb472bc380852cb88",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyruntime-nodejs-0.2.7-ubuntu-22.04-arm64.tar.gz",
            sha256: "2bf39279700ffaaf740359abb47f219de004a5c7e0131df4fcd64f9913c6d0b8",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyruntime-nodejs-0.2.7-macos-26-arm64.tar.gz",
            sha256: "3b3b8f9cd5baa04d67bf1c57841c389697e0e617fffb875b1394ec8ebbf902ac",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyruntime-nodejs-0.2.7-macos-26-x86_64.tar.gz",
            sha256: "61f0d503fee8bd9004b3dd879ce058e888fe1c6702e90ad98e8bc61795fdeb79",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyruntime-nodejs-0.2.7-macos-15-arm64.tar.gz",
            sha256: "0a3c6883c96f50905ad43e624b5fc36248e3fb1b9cb2a03739da75b11260cb96",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyruntime-nodejs-0.2.7-macos-15-x86_64.tar.gz",
            sha256: "0a7240fc04b8b2c34b4bceba9fdd37ee33a76c1456ecc8a087b3b6077a2ec8c2",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyruntime-nodejs-0.2.7-windows-2025-x86_64.zip",
            sha256: "6cda4d0d039b0ee15e1dc1b9bf2f742ac106c5164ceaeccb6d3bb662a729649d",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyruntime-nodejs-0.2.7-windows-2022-x86_64.zip",
            sha256: "033406fe1609cf4161234b0d51257f457f5ba88944aa2f6d2fcd3114b263403d",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyruntime-nodejs-0.2.7-windows-11-arm64.zip",
            sha256: "d15b41caad14346635cdbdd3b0b7777e854e874a22b4bcbc982032e03c5a3665",
        },
    ],
    load: LoadPolicy::Lazy,
};

/// The `tinyruntime-python` module: the Python half of the router's knowledge.
///
/// Answers which host interpreters count, which standalone build to install, and
/// what a warm Python worker is. It installs nothing itself.
///
/// Released from its own repository (own version line) against the router's source pin; see scripts/ci/module-provider-pins.json.
pub(crate) const TINYRUNTIME_PYTHON: ModuleRecord = ModuleRecord {
    id: "tinyruntime-python",
    description: "Python runtime provider for tinyruntime",
    bus_name: "ai.tinyhumans.runtime.python.Provider",
    object_path: "/ai/tinyhumans/runtime/python/Provider",
    version: "0.2.7",
    release_url: "https://github.com/tinyhumansai/tinyruntime-python/releases/tag/v0.2.7",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyruntime-python-0.2.7-ubuntu-24.04-x86_64.tar.gz",
            sha256: "ad1b0b25dea5cbb80e474a5fe9deaed47d8b2f31a1430f3ac00f71e20429ae74",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyruntime-python-0.2.7-ubuntu-24.04-arm64.tar.gz",
            sha256: "dd234eb5b229b7fec8fc7869036e109afd245b75cc52e34ae1ae32031a73976a",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyruntime-python-0.2.7-ubuntu-22.04-x86_64.tar.gz",
            sha256: "009b3e7cd1e845ee9733de73083f64b5920aacf3f494510176b4998e1b031905",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyruntime-python-0.2.7-ubuntu-22.04-arm64.tar.gz",
            sha256: "1be35fafc0580869fdc136255c800325029a205b13a47259332c9a1e4962f663",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyruntime-python-0.2.7-macos-26-arm64.tar.gz",
            sha256: "def3cc6aaa71fa39de02372f6f67cc550f19fd8fbc6dc9e1df38ffa1bd3dfbc5",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyruntime-python-0.2.7-macos-26-x86_64.tar.gz",
            sha256: "81ede9fab71c955bb46a0d1ce005b24763bb3eb38f5d9acb2df3cc9889a3bd60",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyruntime-python-0.2.7-macos-15-arm64.tar.gz",
            sha256: "737cf83fb461862c07cfac99c092a27ccfa245dc3a968a7af2f2ce8d94f4473d",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyruntime-python-0.2.7-macos-15-x86_64.tar.gz",
            sha256: "f37bd1485d18bb9ba9545a402b13314a30265803ae58006b0bbaec040becabc7",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyruntime-python-0.2.7-windows-2025-x86_64.zip",
            sha256: "d449fe10eda0249815196a46d9055d102bed96ead9ef4659b3ef7c6063651a68",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyruntime-python-0.2.7-windows-2022-x86_64.zip",
            sha256: "15c338b8960fed83dcd94a3d0f2aadfa6faa6064938d79a7b659eda7c4e3e4a5",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyruntime-python-0.2.7-windows-11-arm64.zip",
            sha256: "27d9dc021a03990a20648345c232becb1d2b48f361b00a2d656b0549d6bbc33d",
        },
    ],
    load: LoadPolicy::Lazy,
};
