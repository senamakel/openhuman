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
/// remote tool definitions, the `mcp_clients` / `mcp_setup` RPC surface, the
/// agent-facing tools, and the proxy *scoping* decision all belong to this
/// application's threat model, not to a protocol client. `tinymcp-bus` carries
/// the vocabulary; this table says which bytes may speak it.
pub(crate) const TINYMCP: ModuleRecord = ModuleRecord {
    id: "tinymcp",
    description: "Model Context Protocol client: transports, registry, and the write-audit log",
    bus_name: "ai.tinyhumans.tinymcp.Mcp",
    object_path: "/ai/tinyhumans/tinymcp/Mcp",
    version: "0.3.2",
    release_url: "https://github.com/tinyhumansai/tinymcp/releases/tag/v0.3.2",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinymcp-0.3.2-ubuntu-24.04-x86_64.tar.gz",
            sha256: "8bb03dcec777fbd52fedf678dafc04e44afeabc453b3459aace76e721bde7450",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinymcp-0.3.2-ubuntu-24.04-arm64.tar.gz",
            sha256: "cdb06140a3d763c6137dc8470a6896f30707909bc6ac896088391fece220e284",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinymcp-0.3.2-ubuntu-22.04-x86_64.tar.gz",
            sha256: "879de1fb22e4b0b9383638ef00d207ed580a23c6b1fbc85a96b9d405c7e4273d",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinymcp-0.3.2-ubuntu-22.04-arm64.tar.gz",
            sha256: "324a448f1fd3b564f9c3892fe48f96415cd1c3a33f2c234e3c805410136fe7e2",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinymcp-0.3.2-macos-26-arm64.tar.gz",
            sha256: "dd952d4bdf865e9a8b5b358267f7f0c0895d15e9d657c5fc82f15f48f0b281eb",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinymcp-0.3.2-macos-26-x86_64.tar.gz",
            sha256: "11d284c1f9b194c5ac3865656e19b4d4ed3ca91a70e28359f14d13ac73101b1c",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinymcp-0.3.2-macos-15-arm64.tar.gz",
            sha256: "fc86f823719d305de6abc321d88a5b455517c4a6945135af15f7fbc2a3fca403",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinymcp-0.3.2-macos-15-x86_64.tar.gz",
            sha256: "7aec3ab842a7b2c6162d98021873416705b0da5c685ae0c0d8d3792684c0a530",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinymcp-0.3.2-windows-2025-x86_64.zip",
            sha256: "d0defc7df1f4bf4084ebaa1c373316e44f51d27f0ce32b35ac26937905fddda1",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinymcp-0.3.2-windows-2022-x86_64.zip",
            sha256: "71a35710fa45dc07c4f3d24074a189e5cd2ebff678276a57c7b25d907353fe3e",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinymcp-0.3.2-windows-11-arm64.zip",
            sha256: "cd31640774b27adf0472aaf0ad43f3a3c3e9d1b716624b40816c71240abdfa9e",
        },
    ],
    load: LoadPolicy::Lazy,
};

pub(crate) const TINYCONNECTORS: ModuleRecord = ModuleRecord {
    id: "tinyconnectors",
    description: "OAuth connector integrations: accounts, actions, triggers, and record sync",
    bus_name: "ai.tinyhumans.connectors.Composio",
    object_path: "/ai/tinyhumans/connectors/Composio",
    version: "0.10.0",
    release_url: "https://github.com/tinyhumansai/tinyconnectors/releases/tag/v0.10.0",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyconnectors-0.10.0-ubuntu-24.04-x86_64.tar.gz",
            sha256: "fe11f7c0bf06d4826f640685fb824352f075d6b0ec5415b286d761fd4ba371f5",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyconnectors-0.10.0-ubuntu-24.04-arm64.tar.gz",
            sha256: "7b0c3fb3a1ccbcb015c7e4fa7df836265ea8d4648492c1d0481df409ee0dac67",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyconnectors-0.10.0-ubuntu-22.04-x86_64.tar.gz",
            sha256: "53475d279d420b289eea828e0d860e19f291104d19ef9262c2ea826db8c55f61",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyconnectors-0.10.0-ubuntu-22.04-arm64.tar.gz",
            sha256: "c641a261e947bb5e6687cdd0e54ce8493d8390460bad9ee38c00ab3a676d6cc4",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyconnectors-0.10.0-macos-26-arm64.tar.gz",
            sha256: "805f77b475f660311cb06027f3cb707aa76cc53b4138f3bd54e333b73d2ae0c5",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyconnectors-0.10.0-macos-26-x86_64.tar.gz",
            sha256: "8aad88fc729e28abd22e649f9f7a996a6d2149fc44502bf01e5b8457045610e0",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyconnectors-0.10.0-macos-15-arm64.tar.gz",
            sha256: "0b07a5d051f2539c7f332432726342f7b7b6b4b86c8dd7eba1cec0b74777901d",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyconnectors-0.10.0-macos-15-x86_64.tar.gz",
            sha256: "c85556cb125f40c726633e044696cc222fb71b399bf51f0e55bcb0ae583a617d",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyconnectors-0.10.0-windows-2025-x86_64.zip",
            sha256: "29ae0caa048f55fd3b2d34a0885b56210e6a10fad529dce582d65d9680fc0ba7",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyconnectors-0.10.0-windows-2022-x86_64.zip",
            sha256: "dab592b19f75e8cbd7c82c30069df06c5b57923d444b4eacc6dbc4afd38b3f92",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyconnectors-0.10.0-windows-11-arm64.zip",
            sha256: "688edc6fc21fabf7a262b38e9b90d7a1cd81eebf861e8a4b3fa91149b007a08e",
        },
    ],
    // Lazy: a user with no connected accounts should not pay to load it, and
    // most sessions never touch a connector. Safe even signed out — the module
    // loads without configuration and still answers the capability members.
    load: LoadPolicy::Lazy,
};
