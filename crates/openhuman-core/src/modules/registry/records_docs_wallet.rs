//! Registry records for the `tinydocs` and `tinywallet` modules.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

/// The `tinydocs` module: `.docx` / `.pptx` synthesis and `.pdf` extraction.
///
/// Lazy, because a user who never asks for a document should not pay a download,
/// a `dlopen`, and the resident cost of a library that is never unloaded.
pub(crate) const TINYDOCS: ModuleRecord = ModuleRecord {
    id: "tinydocs",
    description: "Document synthesis (.docx, .pptx) and PDF text extraction",
    bus_name: "ai.tinyhumans.tinydocs.Documents",
    object_path: "/ai/tinyhumans/tinydocs/Documents",
    version: "0.1.15",
    release_url: "https://github.com/tinyhumansai/tinydocs/releases/tag/v0.1.15",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinydocs-module-0.1.15-ubuntu-24.04-x86_64.tar.gz",
            sha256: "15a425fb336559bc87722ea0208fe911afd915eda33df9155ce08fe1e737d3a1",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinydocs-module-0.1.15-ubuntu-24.04-arm64.tar.gz",
            sha256: "b1ea63930860624ee1b221cb8669e4435f5551692634ad3911ad9d94d9407aa5",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinydocs-module-0.1.15-ubuntu-22.04-x86_64.tar.gz",
            sha256: "a0f6991e9feeb29a2cbf7a802e46e27232c028517f00606c56a57383c00ba83b",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinydocs-module-0.1.15-ubuntu-22.04-arm64.tar.gz",
            sha256: "c9e66d54c39108cf125d37ba6c1d36cde6f702901a0ba5dcb007e8c6f8f8fd63",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinydocs-module-0.1.15-macos-26-arm64.tar.gz",
            sha256: "b1fdcce5debb275220b1deaea4b90c84daea212496d08a8af95fd7c1d02aa32c",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinydocs-module-0.1.15-macos-26-x86_64.tar.gz",
            sha256: "cbef909e15ef48151feb90746b23c42a2a54ee0cabaa48ee1324c8f5e584f768",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinydocs-module-0.1.15-macos-15-arm64.tar.gz",
            sha256: "979fc81cc8c73ca52afddb2bb363d126f2cabdc0875b0fe954b4bd2d58a308a9",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinydocs-module-0.1.15-macos-15-x86_64.tar.gz",
            sha256: "3094a320870183ca77629f4132ab99f2a411276d3111002b0dd25028904e2517",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinydocs-module-0.1.15-windows-2025-x86_64.zip",
            sha256: "8e7ee6ad41d5b4d39dc54c18802079d4a0ba5757acbf3b3c3b8aef3e0754afdf",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinydocs-module-0.1.15-windows-2022-x86_64.zip",
            sha256: "70bcf4ab948d19c99965c914de44fa9819e57a12cc984217ce3b1851b7584888",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinydocs-module-0.1.15-windows-11-arm64.zip",
            sha256: "f43dea943fab9179c827df5718562a7263ea73f35d915660f9d9fdeadc7de347",
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
    version: "0.5.1",
    release_url: "https://github.com/tinyhumansai/tinywallet/releases/tag/v0.5.1",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinywallet-module-0.5.1-ubuntu-24.04-x86_64.tar.gz",
            sha256: "60ae46bc18b08671d7646ad01affa7f878a4b035a050a47a80353bca5dffc276",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinywallet-module-0.5.1-ubuntu-24.04-arm64.tar.gz",
            sha256: "9f516cb30d36314c72b49c30ad81661da2435a40a064e99583658ba99ed9db2a",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinywallet-module-0.5.1-ubuntu-22.04-x86_64.tar.gz",
            sha256: "88b63685cab8a622416f24f1ad569153f249d6d74732ff33c79e4021cf64a611",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinywallet-module-0.5.1-ubuntu-22.04-arm64.tar.gz",
            sha256: "6c86be45fd260690a93f36024abc9d4f777c30233c70b0363bc23bd25dc4fdfb",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinywallet-module-0.5.1-macos-26-arm64.tar.gz",
            sha256: "4e517d4a3440c2aad852cf5ec23d12863dc9748ecad256dc1578b9f21f4f0ace",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinywallet-module-0.5.1-macos-26-x86_64.tar.gz",
            sha256: "94a6270d07aa0f0788312383552c1baec26ba2db6856c1ab425ddafc4d61b3bb",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinywallet-module-0.5.1-macos-15-arm64.tar.gz",
            sha256: "95e1f1905e0c358ae03b448c11b8c8a413d67951fd02b9f0d78f811f7e5e3037",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinywallet-module-0.5.1-macos-15-x86_64.tar.gz",
            sha256: "55973b9a5b2c0cea8ddd380a3b9ece65c09485faeea2d1cc1e6846b8e888828a",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinywallet-module-0.5.1-windows-2025-x86_64.zip",
            sha256: "96037a6c660e31b747898976feba5c73bd05a47a34b1c9d1f125c215cffde5ec",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinywallet-module-0.5.1-windows-2022-x86_64.zip",
            sha256: "256282d33a832f19cfae3309194f08a602676f4f1c9e52fb2ced998216882f4c",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinywallet-module-0.5.1-windows-11-arm64.zip",
            sha256: "8b56db13123abe43035d339521323cb74969fedc5d57df2d591153dd3afec4d6",
        },
    ],
    load: LoadPolicy::Lazy,
};
