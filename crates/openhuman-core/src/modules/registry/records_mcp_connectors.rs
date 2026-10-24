//! Registry records for the `tinymcp` and `tinyconnectors` modules.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset, SourceSpec};

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
    bus_name: tinymcp_bus::names::INTERFACE,
    object_path: tinymcp_bus::names::OBJECT_PATH,
    version: "0.20.2",
    release_url: "https://github.com/tinyhumansai/tinymcp/releases/tag/v0.20.2",
    assets: &[
        PlatformAsset {
            host_key: "archlinux-rolling-arm64",
            archive: "tinymcp-0.20.2-archlinux-arm64.tar.gz",
            sha256: "130292bd749ce4e075642966e827494dc3f645cd310bfd993579b65645d757eb",
        },
        PlatformAsset {
            host_key: "archlinux-rolling-x86_64",
            archive: "tinymcp-0.20.2-archlinux-x86_64.tar.gz",
            sha256: "2395b184ef39b614914c4cf9a928733617a5b441c6de78e461dcc6c6989f21c4",
        },
        PlatformAsset {
            host_key: "fedora-43-arm64",
            archive: "tinymcp-0.20.2-fedora-43-arm64.tar.gz",
            sha256: "1e3c747a255de2503476f600dfd18598f4f857c74998f96c97fe8e3ae2f3eff5",
        },
        PlatformAsset {
            host_key: "fedora-43-x86_64",
            archive: "tinymcp-0.20.2-fedora-43-x86_64.tar.gz",
            sha256: "8e295d0576100535dd4feebdbceb6114fce6670656ee3f965f26ca20ef6347c3",
        },
        PlatformAsset {
            host_key: "fedora-44-arm64",
            archive: "tinymcp-0.20.2-fedora-44-arm64.tar.gz",
            sha256: "86948bdb4d2a9928bc972b4490af5ab289dc7bca33c7198c9df674ccce966be3",
        },
        PlatformAsset {
            host_key: "fedora-44-x86_64",
            archive: "tinymcp-0.20.2-fedora-44-x86_64.tar.gz",
            sha256: "2652a53066843e4a7d379bc5584e95566f2fa80dd29c9c916408022ca37b5bd0",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinymcp-0.20.2-macos-15-arm64.tar.gz",
            sha256: "369c875740741a7f60da6a29e55e6aad97b65a067190c90aba89b68401721a04",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinymcp-0.20.2-macos-15-x86_64.tar.gz",
            sha256: "0cd9af717a68851e1030656558664a48bc68cf75a2bd1e4cf6714195cf295f76",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinymcp-0.20.2-macos-26-arm64.tar.gz",
            sha256: "fe69bcfae6634a6d8af67b6496f7f53eaa9361f14ee1706f0a0176be6c550670",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinymcp-0.20.2-macos-26-x86_64.tar.gz",
            sha256: "3303a4bd0a40d1400f44691adf8c412c64999f804913839643c29589664e4ded",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinymcp-0.20.2-ubuntu-22.04-arm64.tar.gz",
            sha256: "cc96d0dbaffdb33de1727f17298ea7e7e9fb53a63199732554340133c5920b2d",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinymcp-0.20.2-ubuntu-22.04-x86_64.tar.gz",
            sha256: "f580249fe9f4b9e2587910e686a3bb0999c63ff461aaed9bd812ce7738a14a75",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinymcp-0.20.2-ubuntu-24.04-arm64.tar.gz",
            sha256: "386c0cd318da9e5c88c91fba21cfacbf2b5096768ce3fe609930eca73ee8c5d3",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinymcp-0.20.2-ubuntu-24.04-x86_64.tar.gz",
            sha256: "63b9bf334f3c1e0dbf7db795e33c18ab05849da72c83837bc58ac93b81e5668f",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinymcp-0.20.2-windows-11-arm64.zip",
            sha256: "bbd2c09d0cc4b8acd7af8ba985151d83c87c8b454748f35db44d22027099fa84",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinymcp-0.20.2-windows-2022-x86_64.zip",
            sha256: "51e5c3a435204913692fe006ed6da0e425a122536f9e2f888ba1e9f1d5400da0",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinymcp-0.20.2-windows-2025-x86_64.zip",
            sha256: "e7611602514858e6c3e9bbf942adcd4ea3f9d16d3fcfea10444a5a21b1a77eda",
        },
    ],
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinymcp",
        manifest: "crates/tinymcp/Cargo.toml",
        package: "tinymcp",
        lib_name: "tinymcp",
        features: &[],
    },
};

pub(crate) const TINYCONNECTORS: ModuleRecord = ModuleRecord {
    id: "tinyconnectors",
    description: "OAuth connector integrations: accounts, actions, and triggers",
    bus_name: tinyconnectors_bus::names::INTERFACE,
    object_path: tinyconnectors_bus::names::OBJECT_PATH,
    version: "0.21.0",
    release_url: "https://github.com/tinyhumansai/tinyconnectors/releases/tag/v0.21.0",
    assets: &[
        PlatformAsset {
            host_key: "archlinux-arm64",
            archive: "tinyconnectors-0.21.0-archlinux-arm64.tar.gz",
            sha256: "d147513762e370f683b47f50c58ec9b68b7a62e19ed15759acc854c3e24da3ad",
        },
        PlatformAsset {
            host_key: "archlinux-x86_64",
            archive: "tinyconnectors-0.21.0-archlinux-x86_64.tar.gz",
            sha256: "1672f0299d80b7436dfb485ded0d0db8734482a62bae699179336b3267557ee4",
        },
        PlatformAsset {
            host_key: "fedora-43-arm64",
            archive: "tinyconnectors-0.21.0-fedora-43-arm64.tar.gz",
            sha256: "7993d561bc350d67ab60f8f5c31ed2705aca376fcd34c5a6c61cf127b63b76ac",
        },
        PlatformAsset {
            host_key: "fedora-43-x86_64",
            archive: "tinyconnectors-0.21.0-fedora-43-x86_64.tar.gz",
            sha256: "5fd3b94e49d0d45fb4e2a92a1a668a7a0d5cf8d3a08f6843c6381353f4209e5c",
        },
        PlatformAsset {
            host_key: "fedora-44-arm64",
            archive: "tinyconnectors-0.21.0-fedora-44-arm64.tar.gz",
            sha256: "003b09f5d71c170e72077f046fff429f8c8b61fe6031668b61fc736a22c37457",
        },
        PlatformAsset {
            host_key: "fedora-44-x86_64",
            archive: "tinyconnectors-0.21.0-fedora-44-x86_64.tar.gz",
            sha256: "d95a6bc885d78149bb5d26dce4b291a46b6ffd75d2ce8c6279ff36266448f7ce",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyconnectors-0.21.0-macos-15-arm64.tar.gz",
            sha256: "cf6135caf054d40a5af5b001cc9e549fbe63b1a05f65482d482025bc01cdf2a7",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyconnectors-0.21.0-macos-15-x86_64.tar.gz",
            sha256: "e8fff9402d4ba9453d7cf518e8bea94edae49da8709279e8aa78b432009e0116",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyconnectors-0.21.0-macos-26-arm64.tar.gz",
            sha256: "ca835cbb88e12de0678cc85be3a99cc6e42e415fa73a251c3b5afc344aee4a1e",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyconnectors-0.21.0-macos-26-x86_64.tar.gz",
            sha256: "59060a2f3c7aa71363f7c30b618d72b7181179459ba349f8ec0a9fa536919f44",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyconnectors-0.21.0-ubuntu-22.04-arm64.tar.gz",
            sha256: "f9cc5b782436f3301fe06d62aed87c479ebd4c6745f0b26ea138825d5904f0f9",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyconnectors-0.21.0-ubuntu-22.04-x86_64.tar.gz",
            sha256: "f1210cc9ae65078d7f2604a4dc5dbe57a53b49a1e5017dee5d6e14e57b02de64",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyconnectors-0.21.0-ubuntu-24.04-arm64.tar.gz",
            sha256: "99e8ef19397aa04684d1948b46d94eb94c5fa192ffbf3e47cbe6aedd1fd069b1",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyconnectors-0.21.0-ubuntu-24.04-x86_64.tar.gz",
            sha256: "cfb40dec770adb4a4b2d2db4161f3301f8a14fe8fa7dbb0683b6d579ff2ed2fe",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyconnectors-0.21.0-windows-11-arm64.zip",
            sha256: "49f26d501dce7bee9191f594d782cc4f5f7ff6d0ba9e710ab6413194fdc0c371",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyconnectors-0.21.0-windows-2022-x86_64.zip",
            sha256: "3e96e044e6a310c8e97c2c598198db79b26dca5cee597a7aeb51097d4be119ae",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyconnectors-0.21.0-windows-2025-x86_64.zip",
            sha256: "d7bfe7141ebedfe406db16fc0df0d93f6f658cb0cdbad0415b6c1f332c5167b7",
        },
    ],
    // Lazy: a user with no connected accounts should not pay to load it, and
    // most sessions never touch a connector. Safe even signed out — the module
    // loads without configuration and still answers the capability members.
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinyconnectors",
        manifest: "crates/tinyconnectors/Cargo.toml",
        package: "tinyconnectors",
        lib_name: "tinyconnectors",
        features: &[],
    },
};
