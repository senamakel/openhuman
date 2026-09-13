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
    version: "0.2.4",
    release_url: "https://github.com/tinyhumansai/tinyruntime/releases/tag/v0.2.4",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyruntime-0.2.4-ubuntu-24.04-x86_64.tar.gz",
            sha256: "c0b24429d345a8a62b448de420d98fbef2f31c070ff5d5b1b378d7417ef528ae",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyruntime-0.2.4-ubuntu-24.04-arm64.tar.gz",
            sha256: "4b23925dbd0f6646ec67927909a7e86582332d70a785ddb707f3b043fe5f9664",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyruntime-0.2.4-ubuntu-22.04-x86_64.tar.gz",
            sha256: "f67923a9a84d3924ccb2965fba68fe0961f71c10e1f61888f958deded3b6ba83",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyruntime-0.2.4-ubuntu-22.04-arm64.tar.gz",
            sha256: "61b89164633fa5c234a454559f54ac14c41ad35a7c04f779c3c65daae69f3401",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyruntime-0.2.4-macos-26-arm64.tar.gz",
            sha256: "43342102c5c85af1b0e839b8e3c732db47aa5e04bcc9b6d2c4d6657e4a118efc",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyruntime-0.2.4-macos-26-x86_64.tar.gz",
            sha256: "dd37d959c0d3d721e2654e7c3af4ccd4f1f771b4ac9a83db13dbae761331d10b",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyruntime-0.2.4-macos-15-arm64.tar.gz",
            sha256: "ae4e7577e2031e196dce6b604b821450bb0a0d8610f82a68887a6f0ea2e3db31",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyruntime-0.2.4-macos-15-x86_64.tar.gz",
            sha256: "90168f2be16717f10cf1a27337343df697b0d502d52250342184b0c16380c20a",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyruntime-0.2.4-windows-2025-x86_64.zip",
            sha256: "7556dc37998c73fa0141aa8b737245b44e06473fc115cdde0eaa861804b1dfe1",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyruntime-0.2.4-windows-2022-x86_64.zip",
            sha256: "b1f4679705f83e7627407fe108470b81d9dcfb0f978d5eb11de49e8ff1b9fdc9",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyruntime-0.2.4-windows-11-arm64.zip",
            sha256: "68bc59305c230f3d7b97722ecc009a3a25d55398398a51261049c190c52571a9",
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
    version: "0.2.2",
    release_url: "https://github.com/tinyhumansai/tinyruntime-nodejs/releases/tag/v0.2.2",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyruntime-nodejs-0.2.2-ubuntu-24.04-x86_64.tar.gz",
            sha256: "60bebfacfaccc5c899044fe542a07b1b2ef74ffeeca5d7f53ef0338b6dab4865",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyruntime-nodejs-0.2.2-ubuntu-24.04-arm64.tar.gz",
            sha256: "ff9114e32db29de2a43df83e7d8b330926d5862cdb50ca20adc863d5d99becaf",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyruntime-nodejs-0.2.2-ubuntu-22.04-x86_64.tar.gz",
            sha256: "3f25a17d41226fa8cc56cd9f5f5bd447bff4b9f55c1bd68d7bf8ebbf10575aaa",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyruntime-nodejs-0.2.2-ubuntu-22.04-arm64.tar.gz",
            sha256: "ec271b78487caaea5c5ae1951568a838be49b5df4d362d8855cb27ba243a8c44",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyruntime-nodejs-0.2.2-macos-26-arm64.tar.gz",
            sha256: "394d160e8de754e09121a52ae6a4b5a7b440c0035fb52cbdaa2dfe7ee523b7b0",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyruntime-nodejs-0.2.2-macos-26-x86_64.tar.gz",
            sha256: "bbde43f8d839aacb34f735bbde2e8f56207a1a49fb5b07732a3be7b486243ce3",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyruntime-nodejs-0.2.2-macos-15-arm64.tar.gz",
            sha256: "83ea9c8ea1b43dc4e98cb585e98d254080c2070092b3c1458f19012df5ea3cd8",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyruntime-nodejs-0.2.2-macos-15-x86_64.tar.gz",
            sha256: "6bdb686d1e857d6c28a49ab2ab87785d8c4fecbf7ef62ad218d7b3e159e2339a",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyruntime-nodejs-0.2.2-windows-2025-x86_64.zip",
            sha256: "36aab2547fbb7f336e15ecb66768661a4bd35f3da6179fc3efcd47bbb8d0df96",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyruntime-nodejs-0.2.2-windows-2022-x86_64.zip",
            sha256: "0beaf8ee4765b10f1d12d0ee0c872209935fa48184424842aa6fd299a6e3f5a8",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyruntime-nodejs-0.2.2-windows-11-arm64.zip",
            sha256: "d47571781dc17edfb0438943fbe2026417d33414904667ade0f9cb6de27e5733",
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
    version: "0.2.2",
    release_url: "https://github.com/tinyhumansai/tinyruntime-python/releases/tag/v0.2.2",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyruntime-python-0.2.2-ubuntu-24.04-x86_64.tar.gz",
            sha256: "8d020d8af32f2735e646e164124a84027d260638a1d3cfa392e7c97de179eca6",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyruntime-python-0.2.2-ubuntu-24.04-arm64.tar.gz",
            sha256: "49fb3458636a8247b9735d80a573538bec8c73f8323e9ad0e2eaf5715b88edf1",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyruntime-python-0.2.2-ubuntu-22.04-x86_64.tar.gz",
            sha256: "4f7e23f6f20df2820489f3cde4445e319c5b4c5285bb37e113112f7d83d37a57",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyruntime-python-0.2.2-ubuntu-22.04-arm64.tar.gz",
            sha256: "89ca7864016bd62d2b247fc791b800acf7bbe8903bf40a12da2396e1396a9f63",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyruntime-python-0.2.2-macos-26-arm64.tar.gz",
            sha256: "2d091cbb29dc9d06996f290eaea8f03cf027e8fc9cff72824b9eae86d7ce5483",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyruntime-python-0.2.2-macos-26-x86_64.tar.gz",
            sha256: "b0ec8c06202bf148463a087920387d3f243761756a570a334af16b9ba473267f",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyruntime-python-0.2.2-macos-15-arm64.tar.gz",
            sha256: "5577ed48e84d35ec07d0de8db29c840e0addcd5e54792a02b714e883a65a7ed8",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyruntime-python-0.2.2-macos-15-x86_64.tar.gz",
            sha256: "e08fb6a06a47fd3a1e4e9ae1b6a52f42f3b78655c5f91f4e5dbd7448d6db19a4",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyruntime-python-0.2.2-windows-2025-x86_64.zip",
            sha256: "e22d5120ae58f9562a9861cd2c84a4d88ac692fa12d283ae047aafbe1a71adcc",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyruntime-python-0.2.2-windows-2022-x86_64.zip",
            sha256: "41f27a63ad1e5cc2559ed2fa11d698a775dad55763c7b5e5c884a3ef14f1a811",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyruntime-python-0.2.2-windows-11-arm64.zip",
            sha256: "0e96e8c0dbf1cfd497c8691928659c9f0bb3bf42a77eaa02bce59547f63b929e",
        },
    ],
    load: LoadPolicy::Lazy,
};
