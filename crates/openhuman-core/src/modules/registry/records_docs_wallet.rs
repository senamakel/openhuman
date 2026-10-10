//! Registry records for the `tinydocs` and `tinywallet` modules.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

/// The `tinydocs` module: document synthesis, bounded extraction, and PDF rendering.
///
/// Lazy, because a user who never asks for a document should not pay a download,
/// a `dlopen`, and the resident cost of a library that is never unloaded.
pub(crate) const TINYDOCS: ModuleRecord = ModuleRecord {
    id: "tinydocs",
    description: "Document synthesis, Office/PDF extraction, and PDF rendering",
    bus_name: "ai.tinyhumans.tinydocs.Documents",
    object_path: "/ai/tinyhumans/tinydocs/Documents",
    version: "0.1.22",
    release_url: "https://github.com/tinyhumansai/tinydocs/releases/tag/v0.1.22",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinydocs-module-0.1.22-ubuntu-24.04-x86_64.tar.gz",
            sha256: "b96f7bd28bed47dd89d17c56dec6d3e98a1053342d0edb54d05487bfa264ab28",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinydocs-module-0.1.22-ubuntu-24.04-arm64.tar.gz",
            sha256: "b43f1f3c8a7d26020d38eb50ab787271d1540205a30ab421f0bf89421ce719fc",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinydocs-module-0.1.22-ubuntu-22.04-x86_64.tar.gz",
            sha256: "22c07a48c6e1f684bd16f309bd1045ff766c52205c7f4df6edec84aadbe0c59a",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinydocs-module-0.1.22-ubuntu-22.04-arm64.tar.gz",
            sha256: "d58ce05f62e5367d0b1f996e1b52c1e73006fac91b97b0104c562481c60c2c58",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinydocs-module-0.1.22-macos-26-arm64.tar.gz",
            sha256: "56086b08fe560b9281db6ee86123bff213c81a7686a549b5dc3d18a5c1a6fbe4",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinydocs-module-0.1.22-macos-26-x86_64.tar.gz",
            sha256: "4ee4ea1c0b4832057d21d975400f08f08ae385a5137ac9335d73dfd5e8cb5dd2",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinydocs-module-0.1.22-macos-15-arm64.tar.gz",
            sha256: "9f2625614fe342609302a09a303a3fd03f1c0c29e4030b103d8d1b03387f7ce0",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinydocs-module-0.1.22-macos-15-x86_64.tar.gz",
            sha256: "04f69b78b2b4c73f407b7faf71dedc1481dd941b0f9a6477159874338b749b13",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinydocs-module-0.1.22-windows-2025-x86_64.zip",
            sha256: "27d70441b427bb94b3ceb30500eea0f86f253ba998d68eaaeb4c62be2569ade8",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinydocs-module-0.1.22-windows-2022-x86_64.zip",
            sha256: "47852edcc57f2b6d626f68310f66fee9bf926833a83b52310671998803924c99",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinydocs-module-0.1.22-windows-11-arm64.zip",
            sha256: "e6f4b174f52e61f643d6c13fc424718c8dcd41c7cee2826a3ec6c20391413396",
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
