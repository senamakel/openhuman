use super::{find, ALL};
use tinybus::module::platform::candidates_for;

#[test]
fn tinycomputer_registry_matches_bus_contract_and_published_release() {
    let desktop = find("tinycomputer").expect("compiled computer module");
    assert_eq!(desktop.bus_name, tinycomputer_bus::names::INTERFACE);
    assert_eq!(desktop.object_path, tinycomputer_bus::names::OBJECT_PATH);
    assert_eq!(desktop.version, "0.14.0");
    assert_eq!(desktop.assets.len(), 17);
    assert_eq!(tinycomputer_bus::CONTRACT_VERSION, (2, 20));
    assert_eq!(
        desktop.asset_for("ubuntu-24.04-x86_64").unwrap().sha256,
        "fe79e93bf45d1ba3cd4212d04872530462a99d3d220da182070d54817e8291c6"
    );
    assert_eq!(
        desktop.asset_for("macos-26-arm64").unwrap().sha256,
        "05872f842caa298380a54ea9108aeca04152a70c78fe7177936f61d21b43bc38"
    );
}

#[test]
#[cfg(feature = "web3")]
fn tinywallet_registry_matches_the_verified_v0241_release_manifest() {
    let wallet = find("tinywallet").expect("compiled TinyWallet module");
    assert_eq!(wallet.version, "0.24.1");
    #[cfg(feature = "web3")]
    assert_eq!(tinywallet_bus::CONTRACT_VERSION, (1, 14));
    assert_eq!(
        wallet.release_url,
        "https://github.com/tinyhumansai/tinywallet/releases/tag/v0.24.1"
    );

    let mut actual = wallet
        .assets
        .iter()
        .map(|asset| (asset.host_key, asset.archive, asset.sha256))
        .collect::<Vec<_>>();
    assert_eq!(wallet.assets.len(), 17);
    let mut published = [
        (
            "archlinux-arm64",
            "tinywallet-module-0.24.1-archlinux-arm64.tar.gz",
            "f0e013d305d35744911e4e88ae391bd5b698ca9b5d4b5965ff2cf7e038022a55",
        ),
        (
            "archlinux-x86_64",
            "tinywallet-module-0.24.1-archlinux-x86_64.tar.gz",
            "e8feeacc13150230db39885471868f3cb983d32cdca3b54076cfa3b7b99ca291",
        ),
        (
            "fedora-43-arm64",
            "tinywallet-module-0.24.1-fedora-43-arm64.tar.gz",
            "a5eda11c12b04838101113bb691809cccaab4f02b63927ed87b9ae23354e8024",
        ),
        (
            "fedora-43-x86_64",
            "tinywallet-module-0.24.1-fedora-43-x86_64.tar.gz",
            "81806447a07fb1e5f2e2e11c44cc3bb673b9583836b468086abda33cac7d74c6",
        ),
        (
            "fedora-44-arm64",
            "tinywallet-module-0.24.1-fedora-44-arm64.tar.gz",
            "1b46b6df147080672ed2f87c9b560f3c4ab1d5edabe16f98e66e3cc1df0556fc",
        ),
        (
            "fedora-44-x86_64",
            "tinywallet-module-0.24.1-fedora-44-x86_64.tar.gz",
            "021973415b5e6ebde7f903e9ecd7727a3511fe22bf5f4452720eb6d6b8898a7f",
        ),
        (
            "macos-15-arm64",
            "tinywallet-module-0.24.1-macos-15-arm64.tar.gz",
            "018451ecd682ca8cdd45667d9d7a10e8200a666f1ce5e76c7784f1ed1fe2a59b",
        ),
        (
            "macos-15-x86_64",
            "tinywallet-module-0.24.1-macos-15-x86_64.tar.gz",
            "8c9887fe1a321e0f13b1af3f9129e6db02186fd791d6769e3a80bae964ddb240",
        ),
        (
            "macos-26-arm64",
            "tinywallet-module-0.24.1-macos-26-arm64.tar.gz",
            "d0896d7fbb8936542cdfc4c0d138a262982d599ec015401a6dbd1d4410a39d3a",
        ),
        (
            "macos-26-x86_64",
            "tinywallet-module-0.24.1-macos-26-x86_64.tar.gz",
            "19409a538aa0fd04f246ebcbaacd4aefc7c55279d49f3d066a44257432617314",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinywallet-module-0.24.1-ubuntu-22.04-arm64.tar.gz",
            "0b7da4811e31e2d7239c62fdb30c8ac4c45667b596b27033f4f3915655fccbe7",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinywallet-module-0.24.1-ubuntu-22.04-x86_64.tar.gz",
            "5b1e62a9fa88939cf1711dc0480a6298d240ce2e49cbd4a448a2f967a2f30255",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinywallet-module-0.24.1-ubuntu-24.04-arm64.tar.gz",
            "e50ffef6a61dfbfffc0be0ad32e8388ee72c8803f63b71f435ddc6c47c337650",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinywallet-module-0.24.1-ubuntu-24.04-x86_64.tar.gz",
            "11f66fa19923f1fd151252294b947becf6eb5edafe6a7750681daf74c028d28c",
        ),
        (
            "windows-11-arm64",
            "tinywallet-module-0.24.1-windows-11-arm64.zip",
            "ad8f432919fa78ca7a0828b8c3ad02fcd1b65855cbaf1f76355dcd0423ff7415",
        ),
        (
            "windows-2022-x86_64",
            "tinywallet-module-0.24.1-windows-2022-x86_64.zip",
            "03badd28042ff3a211a489bafb42ac1fa90bb320de66bb73ef5317c7b4c560d3",
        ),
        (
            "windows-2025-x86_64",
            "tinywallet-module-0.24.1-windows-2025-x86_64.zip",
            "53fc1c72e207769fa471373320b0ca73c067c2f6b32a65e73f8fc7ef18a0fa80",
        ),
    ];
    actual.sort_unstable();
    published.sort_unstable();
    assert_eq!(actual, published);
}

#[test]
fn tinybox_registry_matches_the_published_v0138_release_manifest() {
    let record = find("tinybox").expect("compiled TinyBox module");
    assert_eq!(record.version, "0.1.38");
    assert_eq!(tinybox_bus::CONTRACT_VERSION, (1, 21));
    assert_eq!(
        record.release_url,
        "https://github.com/tinyhumansai/tinybox/releases/tag/v0.1.38"
    );

    let actual = record
        .assets
        .iter()
        .map(|asset| (asset.host_key, asset.archive, asset.sha256))
        .collect::<Vec<_>>();
    let published = [
        (
            "archlinux-arm64",
            "tinybox-0.1.38-archlinux-arm64.tar.gz",
            "0e2b68b98239e8822cbf5e79114e68060639541840a36b51d577c830982100df",
        ),
        (
            "archlinux-x86_64",
            "tinybox-0.1.38-archlinux-x86_64.tar.gz",
            "71c434c09574c21833f5313c3baf4646663dfc277a3b82826830d293287d06c3",
        ),
        (
            "fedora-43-arm64",
            "tinybox-0.1.38-fedora-43-arm64.tar.gz",
            "6c0bfecb1aeb97d42b9319754865d2c8595e6310d75ec61358e9d0131efe4ac6",
        ),
        (
            "fedora-43-x86_64",
            "tinybox-0.1.38-fedora-43-x86_64.tar.gz",
            "d6280f22c8f2c7a7793974d8fb8872285ab8cc2615cdb0b5b21093c85de5075d",
        ),
        (
            "fedora-44-arm64",
            "tinybox-0.1.38-fedora-44-arm64.tar.gz",
            "83617a9497c27b6a2597fc8dc9131de054f7c957f114a09ef9a9c9da837d1524",
        ),
        (
            "fedora-44-x86_64",
            "tinybox-0.1.38-fedora-44-x86_64.tar.gz",
            "daf26b47941f9fc5d0d65327beff3f38efe374241401e8717e216235b4970bef",
        ),
        (
            "macos-15-arm64",
            "tinybox-0.1.38-macos-15-arm64.tar.gz",
            "c226235682e64edd4f8f07eafc71de1e7dfc738a495cce47497cd9f9582ada70",
        ),
        (
            "macos-15-x86_64",
            "tinybox-0.1.38-macos-15-x86_64.tar.gz",
            "ed066d3478015e6f063126e10d51556b0069fd6f7896513aa52dffbd75c0ad78",
        ),
        (
            "macos-26-arm64",
            "tinybox-0.1.38-macos-26-arm64.tar.gz",
            "f7b2448881a4af64ea3994a7c8efa83242c1da8b8e8f8c582bb9f0d4e67c636f",
        ),
        (
            "macos-26-x86_64",
            "tinybox-0.1.38-macos-26-x86_64.tar.gz",
            "82c3058ad9685e3a88d903e4545a3a60c15769992d7f510ba5e3f3a7cedaa68d",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinybox-0.1.38-ubuntu-22.04-arm64.tar.gz",
            "b2c10d32409960f2ef9e07bcfdecbfadda3320c077dbcad521919344a790f238",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinybox-0.1.38-ubuntu-22.04-x86_64.tar.gz",
            "016caee6b9548708508c722dc57760d99099d91d2a41e533166b0f1af0147e28",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinybox-0.1.38-ubuntu-24.04-arm64.tar.gz",
            "873f91ae0b85e78a5613b44fe42d8b562b046e8ea1728c9aad1ff6842b5a699a",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinybox-0.1.38-ubuntu-24.04-x86_64.tar.gz",
            "59ff8602dede10a6e3f749aab6623a7bd653b974ed096bad762dcc76fcf6df9a",
        ),
        (
            "windows-11-arm64",
            "tinybox-0.1.38-windows-11-arm64.zip",
            "7bdab1bf410930267f442fe6511eb0298f05928bac47f57d27e5e749d008c898",
        ),
        (
            "windows-2022-x86_64",
            "tinybox-0.1.38-windows-2022-x86_64.zip",
            "ce2aad5f025b9e886d47696a534339a18a2dfcdc5f5a3facd6c5ff1dfa01fa49",
        ),
        (
            "windows-2025-x86_64",
            "tinybox-0.1.38-windows-2025-x86_64.zip",
            "8188eea326549f594cadad130abb54452cf0191626dfa043d3a43dfd806abeca",
        ),
    ];

    assert_eq!(actual.len(), 17);
    assert_eq!(actual, published);
}
#[test]
fn tinymcp_registry_matches_the_published_v0192_release_manifest() {
    let record = find("tinymcp").expect("compiled TinyMCP module");
    assert_eq!(record.version, "0.19.2");
    assert_eq!(tinymcp_bus::CONTRACT_VERSION, (1, 26));
    assert_eq!(
        record.release_url,
        "https://github.com/tinyhumansai/tinymcp/releases/tag/v0.19.2"
    );
    let actual = record
        .assets
        .iter()
        .map(|asset| (asset.host_key, asset.archive, asset.sha256))
        .collect::<Vec<_>>();
    let published = [
        (
            "archlinux-rolling-arm64",
            "tinymcp-0.19.2-archlinux-arm64.tar.gz",
            "cb357041fa7a3815b913fbf235bb10e9ff4c475e3fbf921bc93d8fff644e55e2",
        ),
        (
            "archlinux-rolling-x86_64",
            "tinymcp-0.19.2-archlinux-x86_64.tar.gz",
            "90bc66c70fb1fb3581ca382e9281fc28bc27c836ec1f65998413a4e8641d4c7e",
        ),
        (
            "fedora-43-arm64",
            "tinymcp-0.19.2-fedora-43-arm64.tar.gz",
            "cc805cc004f6baec0adc62e1153f0368ef17180e816c96a895db1b17726d4a2c",
        ),
        (
            "fedora-43-x86_64",
            "tinymcp-0.19.2-fedora-43-x86_64.tar.gz",
            "3ad7ff3b83922d5cf195dfdea9377ddc6295d7d523467f34f3449df38667bc3d",
        ),
        (
            "fedora-44-arm64",
            "tinymcp-0.19.2-fedora-44-arm64.tar.gz",
            "d4a50e75598c3886527abf8f628d40f96b5a86488df3a59d9bafa1b317cf7576",
        ),
        (
            "fedora-44-x86_64",
            "tinymcp-0.19.2-fedora-44-x86_64.tar.gz",
            "62b0fd7a25128af5922ca1a9a80599b4b16fa53e8646c0ee5ff67babd1cf3cd3",
        ),
        (
            "macos-15-arm64",
            "tinymcp-0.19.2-macos-15-arm64.tar.gz",
            "57cb298306eedc7c3149b1438b174fca2d73ce4bfe9ee68211887f9782c7a1c4",
        ),
        (
            "macos-15-x86_64",
            "tinymcp-0.19.2-macos-15-x86_64.tar.gz",
            "8e9f0cfb7886fbf462238257f056f9d3a2309a94328d79643d4283682648f4dc",
        ),
        (
            "macos-26-arm64",
            "tinymcp-0.19.2-macos-26-arm64.tar.gz",
            "f566ef326b754d08b5579927a8df9c9c4db65cf0236468cdc520fce22163f843",
        ),
        (
            "macos-26-x86_64",
            "tinymcp-0.19.2-macos-26-x86_64.tar.gz",
            "37e3e974af8cdabc40483a6e137480a6d0cf9d3623bc98e7eba6f41ef77bbbf7",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinymcp-0.19.2-ubuntu-22.04-arm64.tar.gz",
            "fe474bce2f4666d0e5c0ccfd6e582a33a9f170bbb8212e491c46fedc0ce5bbe3",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinymcp-0.19.2-ubuntu-22.04-x86_64.tar.gz",
            "9a60527d63a162e4ee4a49b28cc9e52d996e32cebcf354107ec5671759888e92",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinymcp-0.19.2-ubuntu-24.04-arm64.tar.gz",
            "33a9c5d5e1f54b1bd4bf6557695d59de414f7e07062bb052ec26807daa2b47e0",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinymcp-0.19.2-ubuntu-24.04-x86_64.tar.gz",
            "699dc226b2c3c4a91f99e6f456f8a24ec14caeb77b2cd08b2741d9dccabb38a5",
        ),
        (
            "windows-11-arm64",
            "tinymcp-0.19.2-windows-11-arm64.zip",
            "d1a067cf4e0bacec0645a949095124b86df6d967a0e01c3ddeb8ea1559becb49",
        ),
        (
            "windows-2022-x86_64",
            "tinymcp-0.19.2-windows-2022-x86_64.zip",
            "ff01a8e99c9e793248e1479ff4ffd79d5c97283589bbf3ad7c0e48bbcde55a93",
        ),
        (
            "windows-2025-x86_64",
            "tinymcp-0.19.2-windows-2025-x86_64.zip",
            "6a9f9c9c53e908d4868d7060502e4c5cd4ccc7ec6717df058aae14afdebe106e",
        ),
    ];
    assert_eq!(actual, published);
}

#[test]
fn ids_and_bus_names_are_unique() {
    // Two records claiming one bus name is a conflict tinybus would only
    // surface at load time, on whichever one happened to be second.
    for (i, record) in ALL.iter().enumerate() {
        for other in &ALL[i + 1..] {
            assert_ne!(record.id, other.id, "duplicate module id");
            assert_ne!(record.bus_name, other.bus_name, "duplicate bus name");
        }
    }
}

#[test]
fn every_object_path_matches_its_bus_name() {
    // tinybus derives a module's object path from its bus name by replacing
    // dots with slashes, and admission compares the two. A mismatch here is
    // a module that downloads and then refuses to load.
    for record in ALL {
        assert_eq!(
            record.object_path,
            format!("/{}", record.bus_name.replace('.', "/")),
            "{} object path does not match its bus name",
            record.id
        );
    }
}

#[test]
fn every_digest_is_a_lowercase_sha256() {
    // An uppercase or truncated digest is refused by tinybus at download
    // time, which is a slow way to find a typo in this file.
    for record in ALL {
        for asset in record.assets {
            assert_eq!(
                asset.sha256.len(),
                64,
                "{} / {} digest is not 64 characters",
                record.id,
                asset.host_key
            );
            assert!(
                asset
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{} / {} digest is not lowercase hex",
                record.id,
                asset.host_key
            );
        }
    }
}

#[test]
fn every_asset_name_carries_its_host_key_and_a_known_extension() {
    // tinybus selects the asset by exact name and requires a `.tar.gz` or
    // `.zip` archive. Most archive platform stems match their host keys;
    // TinyMCP's release uses the canonical `archlinux` stem for the rolling
    // Arch host keys.
    for record in ALL {
        for asset in record.assets {
            let archive_host_key = if record.id == "tinymcp" {
                asset
                    .host_key
                    .strip_prefix("archlinux-rolling-")
                    .map(|suffix| format!("archlinux-{suffix}"))
                    .unwrap_or_else(|| asset.host_key.to_owned())
            } else {
                asset.host_key.to_owned()
            };
            assert!(
                asset.archive.contains(&archive_host_key),
                "{} asset {} does not name its host key {}",
                record.id,
                asset.archive,
                asset.host_key
            );
            let windows = asset.host_key.starts_with("windows");
            assert_eq!(
                windows,
                asset.archive.ends_with(".zip"),
                "{} asset {} has the wrong archive format for its host",
                record.id,
                asset.archive
            );
            if !windows {
                assert!(asset.archive.ends_with(".tar.gz"));
            }
        }
    }
}

#[test]
fn every_asset_name_carries_the_pinned_version() {
    // The digests and the version have to describe one release; an asset
    // left behind at an older version would download bytes the digest
    // beside it never matched.
    for record in ALL {
        for asset in record.assets {
            assert!(
                asset.archive.contains(record.version),
                "{} asset {} is not from version {}",
                record.id,
                asset.archive,
                record.version
            );
        }
    }
}

#[test]
fn the_release_url_is_a_tag_on_github() {
    // tinybus refuses a URL that is not a tag, because a branch URL names
    // bytes that can change under a digest that was checked once.
    for record in ALL.iter().filter(|record| !record.assets.is_empty()) {
        assert!(
            record
                .release_url
                .starts_with("https://github.com/tinyhumansai/"),
            "{} release url is not an upstream GitHub URL",
            record.id
        );
        assert!(
            record.release_url.contains("/releases/tag/"),
            "{} release url is not a tag",
            record.id
        );
        assert!(
            record.release_url.ends_with(record.version),
            "{} release url does not name version {}",
            record.id,
            record.version
        );
    }
}

/// Every host key `platform` can produce, across the supported triples.
fn every_host_key() -> Vec<String> {
    let hosts = [
        ("linux", "x86_64", Some((2, 39))),
        ("linux", "aarch64", Some((2, 39))),
        ("linux", "x86_64", Some((2, 35))),
        ("linux", "aarch64", Some((2, 35))),
        ("macos", "x86_64", None),
        ("macos", "aarch64", None),
        ("windows", "x86_64", None),
        ("windows", "aarch64", None),
    ];
    let mut keys: Vec<String> = hosts
        .into_iter()
        .flat_map(|(os, arch, glibc)| candidates_for(os, arch, glibc))
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

fn supported_host_keys(_record: &super::ModuleRecord) -> Vec<String> {
    every_host_key()
}

#[test]
fn a_record_that_pins_a_release_covers_every_host_the_platform_table_offers() {
    // The two tables are written independently and would drift silently:
    // `platform` offering a key no release publishes turns a supported host
    // into an "unsupported host" at first use.
    //
    // Scoped to records that pin a release at all. A record with no assets
    // is a module this build knows but has no published artifact for; it
    // loads from a developer build or the module search path, and asserting
    // release coverage for a release that does not exist would only assert
    // that it does not exist. The partial-coverage case — the one that is
    // actually a bug — is caught below.
    for record in ALL.iter().filter(|record| !record.assets.is_empty()) {
        for key in supported_host_keys(record) {
            assert!(
                record.asset_for(&key).is_some(),
                "{} publishes no asset for {key}, which the platform table would ask for",
                record.id
            );
        }
    }
}

#[test]
fn a_record_publishes_for_every_host_or_for_none() {
    // Partial coverage is the drift that hurts: it looks supported until a
    // user on the missing platform reaches the feature. All-or-nothing keeps
    // "not published yet" distinguishable from "published and incomplete".
    for record in ALL {
        let host_keys = supported_host_keys(record);
        let covered = host_keys
            .into_iter()
            .filter(|key| record.asset_for(key).is_some())
            .count();
        assert!(
            covered == 0 || covered == supported_host_keys(record).len(),
            "{} publishes assets for {covered} of {} host keys",
            record.id,
            supported_host_keys(record).len()
        );
    }
}

#[test]
fn find_resolves_known_ids_only() {
    assert!(find("tinymcp").is_some());
    assert!(find("not-a-module").is_none());
}

#[test]
fn tinychannels_registry_matches_scoped_runtime_release() {
    let record = find("tinychannels").expect("compiled TinyChannels module");
    assert_eq!(record.version, "0.1.36");
    assert_eq!(tinychannels_bus::CONTRACT_VERSION, 21);
    assert_eq!(
        tinychannels_bus::module_relay::MODULE_OPERATION_VERSION,
        (1, 21)
    );
    let expected = [
        (
            "macos-15-arm64",
            "tinychannels-module-0.1.36-macos-15-arm64.tar.gz",
            "580c5ebf769b2a72130378f894490a635d180429b0b54326710bcef18d3f0e07",
        ),
        (
            "macos-15-x86_64",
            "tinychannels-module-0.1.36-macos-15-x86_64.tar.gz",
            "77c2f35f027a3d040786dbf4fd760e7359fb007dda7ef2793915cb5447d63e9a",
        ),
        (
            "macos-26-arm64",
            "tinychannels-module-0.1.36-macos-26-arm64.tar.gz",
            "91d5dbdd181b909769c6ea7d72d7b93210842da6af7ea8ce2802264a5678bd7e",
        ),
        (
            "macos-26-x86_64",
            "tinychannels-module-0.1.36-macos-26-x86_64.tar.gz",
            "792439fdf93c9d6c90cbd58c66b37ba7f8cf0130bf4184258675e096890b7f73",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinychannels-module-0.1.36-ubuntu-22.04-arm64.tar.gz",
            "a9c166f223096a6a89dfb395fd88c1a62960e2d174cc6a2246f82fb5a21f307c",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinychannels-module-0.1.36-ubuntu-22.04-x86_64.tar.gz",
            "5c510f268c794eda3897c7d77df8ca2223d70df9147ae594d6bcc9258c090d96",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinychannels-module-0.1.36-ubuntu-24.04-arm64.tar.gz",
            "79bfd2fe7f53504169ab5c62afb9c6c7dc8ff4f183bbd150a673f1c4d7c7612a",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinychannels-module-0.1.36-ubuntu-24.04-x86_64.tar.gz",
            "ae50f7234edb5133db96de63b6918e0b602a269fdcd25620ccd1a4d24eaf1a98",
        ),
        (
            "windows-11-arm64",
            "tinychannels-module-0.1.36-windows-11-arm64.zip",
            "c0449841d4f261612485e58c73cacc4dcdf024eebda6c921eda4f56b75ed0ba6",
        ),
        (
            "windows-2022-x86_64",
            "tinychannels-module-0.1.36-windows-2022-x86_64.zip",
            "9e23a4806f20ac877d1bc2e2c55e77b701011eade707859a26c2d9412136ac03",
        ),
        (
            "windows-2025-x86_64",
            "tinychannels-module-0.1.36-windows-2025-x86_64.zip",
            "6b39da8c6e756b89596ba9342f3d708bc0e0f38ee56381b77209f6d38676a178",
        ),
    ];
    assert_eq!(record.assets.len(), expected.len());
    for (key, archive, sha256) in expected {
        let asset = record.asset_for(key).expect("supported platform asset");
        assert_eq!(asset.archive, archive);
        assert_eq!(asset.sha256, sha256);
    }
}

#[cfg(feature = "voice")]
#[test]
fn voice_capture_release_pins_summary_contract_and_every_platform_digest() {
    let record = find("tinyvoice").unwrap();
    assert_eq!(record.version, "0.3.2");
    assert_eq!(tinyvoice_bus::CONTRACT_VERSION, (1, 10));
    let expected = [
        (
            "macos-15-arm64",
            "tinyvoice-module-0.3.2-macos-15-arm64.tar.gz",
            "316cb2b9b9a1630f2c815744121e3bbc1d0df58364bb012dd6f11f78978ba947",
        ),
        (
            "macos-15-x86_64",
            "tinyvoice-module-0.3.2-macos-15-x86_64.tar.gz",
            "51045754d67386db5b09a19714ec8b1f319ad9896bb3f936325d7935ce5dce0e",
        ),
        (
            "macos-26-arm64",
            "tinyvoice-module-0.3.2-macos-26-arm64.tar.gz",
            "2a4ee8f87229422019adfd28f07ac26d5e2e78165550b04f0e563764a66a429e",
        ),
        (
            "macos-26-x86_64",
            "tinyvoice-module-0.3.2-macos-26-x86_64.tar.gz",
            "4199913e36c12987a82606f98c93e2a85e086aaa17380469a9aa68b719c853d9",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinyvoice-module-0.3.2-ubuntu-22.04-arm64.tar.gz",
            "533a3715f5efad9911e7768e8280bbe724c60785e0ff67939867ae21a20f690e",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinyvoice-module-0.3.2-ubuntu-22.04-x86_64.tar.gz",
            "d173a63a6b20e0441d18d639906ec9c74f96178c50e274ebd32bf94abbf5260b",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinyvoice-module-0.3.2-ubuntu-24.04-arm64.tar.gz",
            "f2145cd4bf729b5b6ad563687b061557d502c786ba935a65433e4338adf75a7a",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinyvoice-module-0.3.2-ubuntu-24.04-x86_64.tar.gz",
            "6ee278898d33f3b3ca7a9bf8a313dc4ab4e207eabf970608d37f56d825587519",
        ),
        (
            "windows-11-arm64",
            "tinyvoice-module-0.3.2-windows-11-arm64.zip",
            "b3cbff9b477495027839b0266201299b0db5cb585a9e41b3110cf46f16c35c99",
        ),
        (
            "windows-2022-x86_64",
            "tinyvoice-module-0.3.2-windows-2022-x86_64.zip",
            "7b7a8a8e04a57adc306b2b840bff43ea6a27090e2b089b74cfcf451f18172456",
        ),
        (
            "windows-2025-x86_64",
            "tinyvoice-module-0.3.2-windows-2025-x86_64.zip",
            "04cf6b21ed7d73375181c6cf5a18fa52a50fb09049031f0926cd46c3bcf33431",
        ),
    ];
    assert_eq!(record.assets.len(), expected.len());
    for (key, archive, digest) in expected {
        let asset = record.asset_for(key).unwrap();
        assert_eq!(asset.archive, archive);
        assert_eq!(asset.sha256, digest);
    }
}

#[test]
fn every_record_names_a_source_manifest_that_exists_in_its_submodule() {
    let vendor = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor");
    for record in ALL {
        let spec = &record.source;
        assert!(!spec.package.is_empty(), "{}: package", record.id);
        assert!(!spec.lib_name.is_empty(), "{}: lib_name", record.id);
        assert_eq!(spec.submodule, record.id, "{}: submodule dir", record.id);
        let manifest = vendor.join(spec.submodule).join(spec.manifest);
        assert!(
            manifest.is_file(),
            "{}: source manifest {} is missing (is the submodule initialised?)",
            record.id,
            spec.manifest
        );
    }
}

#[test]
fn source_artifact_file_names_follow_the_platform_convention() {
    for record in ALL {
        let name = record.source.artifact_filename();
        assert!(name.contains(record.source.lib_name), "{}", record.id);
        if cfg!(target_os = "linux") {
            assert!(name.starts_with("lib") && name.ends_with(".so"), "{name}");
        }
    }
}

#[test]
fn tinysearch_registry_matches_bus_contract_and_published_release() {
    let search = find("tinysearch").expect("compiled search module");
    assert_eq!(search.bus_name, tinysearch_bus::names::INTERFACE);
    assert_eq!(search.object_path, tinysearch_bus::names::OBJECT_PATH);
    assert_eq!(search.version, "0.5.1");
    assert_eq!(tinysearch_bus::CONTRACT_VERSION, (2, 2));
    assert_eq!(
        search.asset_for("ubuntu-24.04-x86_64").unwrap().sha256,
        "f078d63ada0c5051a830d803079051d9dee4c7c08c9b6e61100692ed2c63c887"
    );
}
