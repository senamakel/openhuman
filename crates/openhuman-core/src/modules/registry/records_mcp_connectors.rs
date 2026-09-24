//! Registry records for the `tinymcp` and `tinyconnectors` modules.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset};

/// The `tinymcp` module: the Model Context Protocol client.
///
/// Owns both transports (Streamable HTTP and a subprocess over stdio), the
/// statically declared server set a host puts in its own configuration, the
/// dynamic registry of user-installed servers with its SQLite store, the
/// reconnect supervisor, the browser sign-in flow, and the write-audit log.
///
/// Lazy, because dialing an MCP server is something most sessions never do: a
/// host with no installed servers and no configured ones would otherwise pay a
/// download and a `dlopen` for a capability it never reaches. That differs from
/// the module's own `lazy = false` export hint, which speaks for a host whose
/// servers should be connected the moment it comes up — this host decides when
/// that moment is, and does so on the first ask.
///
/// **What stays out of the module is host policy**, and the split is the same
/// one the contract's own documentation draws: the prompt-injection scan over
/// remote tool definitions, the `mcp_clients` RPC surface, the
/// agent-facing tools, and the proxy *scoping* decision all belong to this
/// application's threat model, not to a protocol client. `tinymcp-bus` carries
/// the vocabulary; this table says which bytes may speak it.
pub(crate) const TINYMCP: ModuleRecord = ModuleRecord {
    id: "tinymcp",
    description: "Model Context Protocol client: transports, registry, and the write-audit log",
    bus_name: "ai.tinyhumans.tinymcp.Mcp",
    object_path: "/ai/tinyhumans/tinymcp/Mcp",
    version: "0.3.3",
    release_url: "https://github.com/tinyhumansai/tinymcp/releases/tag/v0.3.3",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinymcp-0.3.3-ubuntu-24.04-x86_64.tar.gz",
            sha256: "7c49adafbdeed02b5555d35aafb9e0b1d6a27ea6586aa24518d19d58dc21079b",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinymcp-0.3.3-ubuntu-24.04-arm64.tar.gz",
            sha256: "996637662a3406681eae477d2d333312894a9709c83fa179a7aacfe068ba8be0",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinymcp-0.3.3-ubuntu-22.04-x86_64.tar.gz",
            sha256: "82da931e0e5e9e82feb189042ac130fa37f3ae5ffec8d935ddd89b41859ef55c",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinymcp-0.3.3-ubuntu-22.04-arm64.tar.gz",
            sha256: "a69a995808c0098ce80ef9a38d590b829e83abd9370410807cf482f0ae77097a",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinymcp-0.3.3-macos-26-arm64.tar.gz",
            sha256: "5fefcc5a3ec34dbd4bcee70615fff456be9cf28a74c1d06d07f810137facf5d7",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinymcp-0.3.3-macos-26-x86_64.tar.gz",
            sha256: "c04b9ec6746f3317c695cde897a168101a6ea9fe1fe9b5e08a85aba3a2429baa",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinymcp-0.3.3-macos-15-arm64.tar.gz",
            sha256: "67172f3d6be08a57ee5e83039d8100be60132e1bbf32d027f6b2eb150eb59b3f",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinymcp-0.3.3-macos-15-x86_64.tar.gz",
            sha256: "6eac1dd3e55a0e6ad5341c30182f24fd5ea0bba7cce07ecec17c607aea40882c",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinymcp-0.3.3-windows-2025-x86_64.zip",
            sha256: "e9b7dd357deddc3e659eccac1eeb9a9bc018f083b3120de2bb36027b7824cd9b",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinymcp-0.3.3-windows-2022-x86_64.zip",
            sha256: "961cf051976f8f650270764b3ad11ced8067d1a0f953c091ed4b9da524e18051",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinymcp-0.3.3-windows-11-arm64.zip",
            sha256: "312eda791a4cc3fa9e8ef46415c8471ba280aea63534718486a92ee764debe61",
        },
    ],
    load: LoadPolicy::Lazy,
};

pub(crate) const TINYCONNECTORS: ModuleRecord = ModuleRecord {
    id: "tinyconnectors",
    description: "OAuth connector integrations: accounts, actions, triggers, and record sync",
    bus_name: "ai.tinyhumans.connectors.Composio",
    object_path: "/ai/tinyhumans/connectors/Composio",
    version: "0.10.2",
    release_url: "https://github.com/tinyhumansai/tinyconnectors/releases/tag/v0.10.2",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyconnectors-0.10.2-ubuntu-24.04-x86_64.tar.gz",
            sha256: "2be34c4e66b40698b26ce352a4d71cece50a872de805c650de99ac728b2e73b5",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyconnectors-0.10.2-ubuntu-24.04-arm64.tar.gz",
            sha256: "7979662b926d6554910b5b1d51342a59279caef8d85024be37f90cb5df9f5388",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyconnectors-0.10.2-ubuntu-22.04-x86_64.tar.gz",
            sha256: "678008661bf2f892cb941fa4dc8e5d1929d4004de35604163fa395d19c8d5a19",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyconnectors-0.10.2-ubuntu-22.04-arm64.tar.gz",
            sha256: "80bcfbb979e18db7aee3cc2c075dac2a7a391e76b7a3e102dc6d991ab2603831",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyconnectors-0.10.2-macos-26-arm64.tar.gz",
            sha256: "0f9073f873b1bad1d64290d0893e214338c2c5e66bab7a2267d55cd7397bc289",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyconnectors-0.10.2-macos-26-x86_64.tar.gz",
            sha256: "9fd9c3b0ac4776d2f1e4bb583b817ad4c50532f9761b66a2233aa53b16d8ec78",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyconnectors-0.10.2-macos-15-arm64.tar.gz",
            sha256: "0fb2384e03d2afc348647f7779b5782ba3c357f7ca85f1887237a823377c2497",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyconnectors-0.10.2-macos-15-x86_64.tar.gz",
            sha256: "f9eb1812c92e65b78cc50e3cb9e59f78effe2b0dc5f97e55d5873afa969f3f4c",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyconnectors-0.10.2-windows-2025-x86_64.zip",
            sha256: "6b01d1b4d1bb7c5996142b06f7798766cdbed9d75de04a495a3da396848c3ab1",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyconnectors-0.10.2-windows-2022-x86_64.zip",
            sha256: "e8b03c3510d7ef0526b2ac6d58fbe3a341215474373101966651c3db2612f46f",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyconnectors-0.10.2-windows-11-arm64.zip",
            sha256: "501a038f8cfcfc80ad9605379925677b93a747f87e0f8c00b8524218fc776fb9",
        },
    ],
    // Lazy: a user with no connected accounts should not pay to load it, and
    // most sessions never touch a connector. Safe even signed out — the module
    // loads without configuration and still answers the capability members.
    load: LoadPolicy::Lazy,
};
