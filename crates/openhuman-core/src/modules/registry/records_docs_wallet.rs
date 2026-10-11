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

/// The `tinywallet` module: transaction building and assembly for four chains.
///
/// Lazy for the same reason as [`TINYDOCS`], and more so: most sessions never
/// touch a wallet, and this artifact carries `bitcoin` and a native `secp256k1`
/// build that would otherwise be resident for all of them.
///
/// **This host sends it the recovery phrase, over confidential calls, and never
/// derives or signs itself.** All four chains — Bitcoin, EVM, Solana and Tron —
/// derive and sign inside the module. This binary does not link the root
/// `tinywallet` crate at all — it takes `tinywallet-bus`, the wire contract,
/// which carries no `key` gate — nor does it link `k256`; see the note on the
/// `tinywallet-bus` dependency.
///
/// The phrase is only sent to a module tinybus has attested *and* whose digest
/// matches one of the entries below — `super::wallet::attested_proxy` checks
/// this table itself rather than trusting that some check happened.
///
/// The contract also exposes `ExportKey` for downstream hosts that must drive
/// a signer locally; OpenHuman itself does not call it.
///
/// Three releases got here, and the order mattered. v0.2.3 changed no method at
/// all — it was the same module rebuilt against a bus that could attest it.
/// Attestation used to be recorded only from a `modules.toml` beside the
/// artifact, and a release download extracts into a temporary directory that has
/// none, so this module could never be an attested recipient however carefully
/// the digest below was pinned (tinybus#15 fixed that). Only then was it safe
/// for v0.3.0 to add methods that take a secret, and for v0.4.0 to add
/// `SignMessage` for the Solana and x402 encodings the wire contract does not
/// model. Adding them earlier would have made them unreachable in production and
/// reachable in a developer's tree, which is the worst of both.
pub(crate) const TINYWALLET: ModuleRecord = ModuleRecord {
    id: "tinywallet",
    description: "Transaction building and assembly for Bitcoin, EVM, Solana and Tron",
    bus_name: "ai.tinyhumans.tinywallet.Wallet",
    object_path: "/ai/tinyhumans/tinywallet/Wallet",
    version: "0.8.0",
    release_url: "https://github.com/tinyhumansai/tinywallet/releases/tag/v0.8.0",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinywallet-module-0.8.0-ubuntu-24.04-x86_64.tar.gz",
            sha256: "401d9fb2491030907c58778216bafbc78130fe8a486106faaa86432ff22a2bae",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinywallet-module-0.8.0-ubuntu-24.04-arm64.tar.gz",
            sha256: "0444ff0cad34ac0aa7e26a18a84737ca48193b7f6a1452930dcba3e413174b4a",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinywallet-module-0.8.0-ubuntu-22.04-x86_64.tar.gz",
            sha256: "c16f2fdefe1f032e89d68969ccbde762ae17dde85ee1aa0248f7820852ac1008",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinywallet-module-0.8.0-ubuntu-22.04-arm64.tar.gz",
            sha256: "690c99b72655e13aaa1a307e599dff09f77e5470101fbf3331d321d787805885",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinywallet-module-0.8.0-macos-26-arm64.tar.gz",
            sha256: "206e74e88011a7810c88375d96082f1a85036f3d10194aedbc1513d2fe4a39a7",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinywallet-module-0.8.0-macos-26-x86_64.tar.gz",
            sha256: "d4163442c6fdeada5b56a1b08b92d22ff77ec3b21634bff61ac8bf0de1bee1a0",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinywallet-module-0.8.0-macos-15-arm64.tar.gz",
            sha256: "a27f04db2652b09e4b855cb9ba17c9fda90ea65b0eeda6335f78fb9839a5b681",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinywallet-module-0.8.0-macos-15-x86_64.tar.gz",
            sha256: "6778db75763d9f7058228f6beeb75af04b2d2bd74d38b8109e4e310a2dfaca77",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinywallet-module-0.8.0-windows-2025-x86_64.zip",
            sha256: "48ed01c962dfd4ff35010a858f7663105cce9b24179f7e230c472742b63a3e70",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinywallet-module-0.8.0-windows-2022-x86_64.zip",
            sha256: "8010a390a9b27e28ac0e384f398b101f80980bc477312d5055bbb3edacf36e80",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinywallet-module-0.8.0-windows-11-arm64.zip",
            sha256: "d9e685d709755c52f4ba6d1d852da5b237a19f405de95389d8650ba54e231045",
        },
    ],
    load: LoadPolicy::Lazy,
};
