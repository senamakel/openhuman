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
    version: "0.6.0",
    release_url: "https://github.com/tinyhumansai/tinymcp/releases/tag/v0.6.0",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinymcp-0.6.0-ubuntu-24.04-x86_64.tar.gz",
            sha256: "f32a322f180e24cc942e6744f9be9458eebb2e97c1be67795eb51c03b6cecb42",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinymcp-0.6.0-ubuntu-24.04-arm64.tar.gz",
            sha256: "cbf0dfc454198b62b6d44830f215abac78100cb211b103984b6b25dbc3eeba41",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinymcp-0.6.0-ubuntu-22.04-x86_64.tar.gz",
            sha256: "8ffe34e4fa3d7076cef9dcb755182824bdaaadf6f9182dea098c72e99f5a6081",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinymcp-0.6.0-ubuntu-22.04-arm64.tar.gz",
            sha256: "7ec7ad75767910696fe472acfd17808cb4b7259b7b3d477cefe133313524bd75",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinymcp-0.6.0-macos-26-arm64.tar.gz",
            sha256: "4b0c76f358d80bd7f108fd6c66e8cd881a97271eafa393960360fcd8559a1a7c",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinymcp-0.6.0-macos-26-x86_64.tar.gz",
            sha256: "7934a57ca96aa1525cdd14fa4bd31c47d24b38f957401795569f7f2b92cda761",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinymcp-0.6.0-macos-15-arm64.tar.gz",
            sha256: "6fd14c70d5ea1bd140d97b8512f754032afe6e794dfcff6b42d4b925769ce9fe",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinymcp-0.6.0-macos-15-x86_64.tar.gz",
            sha256: "7430221adcb2d8bcda3c010f600df0fcdbe2acf5854d657562601ad561a3fad4",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinymcp-0.6.0-windows-2025-x86_64.zip",
            sha256: "fab6aebe81bc57f86e1c024d77ea0a01fa08f28204ff186bb57c3f581520d62c",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinymcp-0.6.0-windows-2022-x86_64.zip",
            sha256: "b490eee140645da47b6eb5edf579af92e3889f25e40fb76ace579afd4e506a66",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinymcp-0.6.0-windows-11-arm64.zip",
            sha256: "af8eb16669df093e6dda87a8099e78ce67ec90c8262691249a47b93862480e29",
        },
    ],
    load: LoadPolicy::Lazy,
};

pub(crate) const TINYCONNECTORS: ModuleRecord = ModuleRecord {
    id: "tinyconnectors",
    description: "OAuth connector integrations: accounts, actions, and triggers",
    bus_name: "ai.tinyhumans.connectors.Composio",
    object_path: "/ai/tinyhumans/connectors/Composio",
    version: "0.13.1",
    release_url: "https://github.com/tinyhumansai/tinyconnectors/releases/tag/v0.13.1",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyconnectors-0.13.1-ubuntu-24.04-x86_64.tar.gz",
            sha256: "e57a5e3d8253e175e7473a70e1d47f0884991772c3727c9d0d9c7adb511fb1e8",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyconnectors-0.13.1-ubuntu-24.04-arm64.tar.gz",
            sha256: "398429071b59dff977a78e0971455f5e57a374234142253d8d157323e04997ed",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyconnectors-0.13.1-ubuntu-22.04-x86_64.tar.gz",
            sha256: "6503de62d2a499f249fc98264d10e6dc79157a401862307d2380d62fb4815896",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyconnectors-0.13.1-ubuntu-22.04-arm64.tar.gz",
            sha256: "5d9fe6bb8da1202b00f1bed80b8300acf0a471d77ee5b83cb61cb2df064a011b",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyconnectors-0.13.1-macos-26-arm64.tar.gz",
            sha256: "d0bb7832393df931b209157ae9c7d98ecfa59a4f48e070b00e99d8ba3a142f40",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyconnectors-0.13.1-macos-26-x86_64.tar.gz",
            sha256: "8a7239fbae0558d9d4e46e83dcdcb8de2464fb0f50c3ca5b52a0c1705f2a6531",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyconnectors-0.13.1-macos-15-arm64.tar.gz",
            sha256: "d33d1d9e2bc844619122e7d242910d2951febbaaecdb56c75048ff53d4951dc1",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyconnectors-0.13.1-macos-15-x86_64.tar.gz",
            sha256: "bc95e33c1c2ef766973ae8ddaa5c51b72795f06f64e30a99af69d237ab427ba6",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyconnectors-0.13.1-windows-2025-x86_64.zip",
            sha256: "7a1aebe170fe9179be05940c7a01f5971ffa25f02c606f69b7923109104e027e",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyconnectors-0.13.1-windows-2022-x86_64.zip",
            sha256: "9f60d03c5af08b5f1c2f3cc61a479f6427a64522a4ddf08c8113193a6f838930",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyconnectors-0.13.1-windows-11-arm64.zip",
            sha256: "c3dc32d04105345794b6aaaa9e67ed411b4562e31664998e3e50264eaf93de38",
        },
    ],
    // Lazy: a user with no connected accounts should not pay to load it, and
    // most sessions never touch a connector. Safe even signed out — the module
    // loads without configuration and still answers the capability members.
    load: LoadPolicy::Lazy,
};
