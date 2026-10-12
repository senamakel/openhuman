//! Registry records for the `tinydocs` and `tinywallet` modules.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

/// The `tinydocs` module: document synthesis, bounded extraction, image facts,
/// and PDF rendering.
///
/// Lazy, because a user who never asks for a document should not pay a download,
/// a `dlopen`, and the resident cost of a library that is never unloaded.
pub(crate) const TINYDOCS: ModuleRecord = ModuleRecord {
    id: "tinydocs",
    description: "Document synthesis, Office/PDF extraction, image inspection, and PDF rendering",
    bus_name: "ai.tinyhumans.tinydocs.Documents",
    object_path: "/ai/tinyhumans/tinydocs/Documents",
    version: "0.2.0",
    release_url: "https://github.com/tinyhumansai/tinydocs/releases/tag/v0.2.0",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinydocs-module-0.2.0-ubuntu-24.04-x86_64.tar.gz",
            sha256: "2db0db216f6bd25e07d66cd13485672dd2edcd84bed9c4f5ee383f603ced7c95",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinydocs-module-0.2.0-ubuntu-24.04-arm64.tar.gz",
            sha256: "9d77120bceac02ac607192f12884023c8f10a18baecef18acb3694ecb8b42217",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinydocs-module-0.2.0-ubuntu-22.04-x86_64.tar.gz",
            sha256: "d3a84df5f375a9774183dd5c1fd344ac373752b54473a6d2fb611c7cab5142d2",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinydocs-module-0.2.0-ubuntu-22.04-arm64.tar.gz",
            sha256: "fa094051f186e085db8bdc304215aa5553c0f29c958b78cc337623815e62c39a",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinydocs-module-0.2.0-macos-26-arm64.tar.gz",
            sha256: "94553eef6029b61ad135d406000e7ccfb03857e68be03fed990d8912b9b1995f",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinydocs-module-0.2.0-macos-26-x86_64.tar.gz",
            sha256: "b01bba79f64731534ca93c89d045cfdd8cd8d6ce34249810a99e76735e4e3b59",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinydocs-module-0.2.0-macos-15-arm64.tar.gz",
            sha256: "b92f0b31d942f56784bf9fd6897736595f5c1bf39db34a7115e5d9ddafb0fe72",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinydocs-module-0.2.0-macos-15-x86_64.tar.gz",
            sha256: "2f6ff91ff580462a234e7476bb0dc3c6fda975786e2ea920b0a85ece3df760d8",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinydocs-module-0.2.0-windows-2025-x86_64.zip",
            sha256: "e799aca9485cff97eceb4a3f8f6882c5e92abb17e639a68d2f774fcf57f035c2",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinydocs-module-0.2.0-windows-2022-x86_64.zip",
            sha256: "5a271fa3a9c472cdd531d259db92dc04512199ac1cd586fdf31c3af14c3299fb",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinydocs-module-0.2.0-windows-11-arm64.zip",
            sha256: "15541820df3ff69621485846d328495d42d06e7d9b2e73c42eea5af2242a9a4c",
        },
    ],
    load: LoadPolicy::Lazy,
};

/// The `tinywallet` module: native wallet, Web3 quote, and x402 operations.
///
/// Lazy for the same reason as [`TINYDOCS`], and more so: most sessions never
/// touch a wallet, and this artifact carries `bitcoin` and a native `secp256k1`
/// build that would otherwise be resident for all of them.
///
/// **This host sends it the recovery phrase, over confidential calls, and never
/// derives or signs itself.** All four chains — Bitcoin, EVM, Solana and Tron —
/// derive and sign inside the module. Web3 quotes and x402 payments, budgets,
/// and ledger state also live there. The host supplies credential custody,
/// authenticated transports, and exact-facts approval callbacks. This binary
/// depends on `tinywallet-bus` for the wire contract rather than implementation
/// crates.
///
/// The phrase is only sent to a module tinybus has attested *and* whose digest
/// matches one of the entries below — `super::wallet::attested_proxy` checks
/// this table itself rather than trusting that some check happened.
///
/// The contract also exposes `ExportKey` for downstream hosts that must drive
/// a signer locally; OpenHuman itself does not call it.
///
/// The pinned v0.19.0 release carries the native wallet, Web3, and x402 bus
/// operations, including typed configured-endpoint references. Secret-bearing
/// operations remain confidential and require both attestation and a matching
/// pinned digest.
pub(crate) const TINYWALLET: ModuleRecord = ModuleRecord {
    id: "tinywallet",
    description: "Transaction building and assembly for Bitcoin, EVM, Solana and Tron",
    bus_name: "ai.tinyhumans.tinywallet.Wallet",
    object_path: "/ai/tinyhumans/tinywallet/Wallet",
    version: "0.19.0",
    release_url: "https://github.com/tinyhumansai/tinywallet/releases/tag/v0.19.0",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinywallet-module-0.19.0-ubuntu-24.04-x86_64.tar.gz",
            sha256: "55c27a4c85631aee0509e9c74ae1eceb5f3fb3aa43540294bdd4991eac5500af",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinywallet-module-0.19.0-ubuntu-24.04-arm64.tar.gz",
            sha256: "aa79e84f60892f2ff624f42619f928b2699bdbea613f7bc2a0595f769c7f1d68",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinywallet-module-0.19.0-ubuntu-22.04-x86_64.tar.gz",
            sha256: "4511bf4a03f9163f5493472c1b294eccd8087e44efbde6b7e50c2ee4867b707c",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinywallet-module-0.19.0-ubuntu-22.04-arm64.tar.gz",
            sha256: "d981e334615d4a4d2ea003a4c3d59ee8f631d6f52532cfa70dc97143e97a6fe8",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinywallet-module-0.19.0-macos-26-arm64.tar.gz",
            sha256: "bff93e7c33fcf663c0b69117add0e249c461233a58eb5ba2f1b753a9a8944350",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinywallet-module-0.19.0-macos-26-x86_64.tar.gz",
            sha256: "72e1ef5c1047c0ac848abb9cd1cf69e6639ac4e8527496979406922852a4d2d3",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinywallet-module-0.19.0-macos-15-arm64.tar.gz",
            sha256: "15397175e39dffbfe372bb40a4dccd6e03440ed5925043d41df058a335e1427d",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinywallet-module-0.19.0-macos-15-x86_64.tar.gz",
            sha256: "389e68114a4d1243afe04e8143e7f6ebd321c1cc24b54be2d768c7ee8643c7b1",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinywallet-module-0.19.0-windows-2025-x86_64.zip",
            sha256: "17856ed57fb14840c3a4c5cde9167a68d9a8d578ee63728852b894d3d1e8fec1",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinywallet-module-0.19.0-windows-2022-x86_64.zip",
            sha256: "398bce97e33fa44faa3e92eb20cdd448089882233858a952b7710ad4d1806546",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinywallet-module-0.19.0-windows-11-arm64.zip",
            sha256: "f91ef12a88c324a4c890be0ff8114239d835770541ce8cfd6059f015d3720764",
        },
        PlatformAsset {
            host_key: "archlinux-arm64",
            archive: "tinywallet-module-0.19.0-archlinux-arm64.tar.gz",
            sha256: "59863d52205fabe486a16dfbf573a007d19c3237236cee3a8ff236281cdf6ba9",
        },
        PlatformAsset {
            host_key: "archlinux-x86_64",
            archive: "tinywallet-module-0.19.0-archlinux-x86_64.tar.gz",
            sha256: "645679737effe617f566af7349dc7153f29dac21fa7a377a610ccf72054e7454",
        },
        PlatformAsset {
            host_key: "fedora-43-arm64",
            archive: "tinywallet-module-0.19.0-fedora-43-arm64.tar.gz",
            sha256: "c05ff2da4101246b7a50a37a44632a8502a5747f5e7cc416258fb728782d91b6",
        },
        PlatformAsset {
            host_key: "fedora-43-x86_64",
            archive: "tinywallet-module-0.19.0-fedora-43-x86_64.tar.gz",
            sha256: "c4399bbb1c9d3c42545fb78474f548337cfc4a96ebb49ce50912ca4c85dd3aa4",
        },
        PlatformAsset {
            host_key: "fedora-44-arm64",
            archive: "tinywallet-module-0.19.0-fedora-44-arm64.tar.gz",
            sha256: "d17075dd175d0bd3bebe31350a3bcc58320c698f4c54f69691f85dd54297fdb9",
        },
        PlatformAsset {
            host_key: "fedora-44-x86_64",
            archive: "tinywallet-module-0.19.0-fedora-44-x86_64.tar.gz",
            sha256: "ef64e56e536a2661ae0ddcd799632819c02f391a5e7f98431adc184f7df45adb",
        },
    ],
    load: LoadPolicy::Lazy,
};
