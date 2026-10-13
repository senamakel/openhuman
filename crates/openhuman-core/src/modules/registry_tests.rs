use super::{find, ALL};
use tinybus::module::platform::candidates_for;

#[test]
fn tinycomputer_registry_matches_bus_contract_and_published_release() {
    let desktop = find("tinycomputer").expect("compiled computer module");
    assert_eq!(desktop.bus_name, tinycomputer_bus::names::INTERFACE);
    assert_eq!(desktop.object_path, tinycomputer_bus::names::OBJECT_PATH);
    assert_eq!(desktop.version, "0.12.0");
    assert_eq!(desktop.assets.len(), 17);
    assert_eq!(tinycomputer_bus::CONTRACT_VERSION, (2, 16));
    assert_eq!(
        desktop.asset_for("ubuntu-24.04-x86_64").unwrap().sha256,
        "ab6f0cdc859dc16bbe8683666a801d25458e262951f6cdd0d1d859ef7dafdd2a"
    );
    assert_eq!(
        desktop.asset_for("macos-26-arm64").unwrap().sha256,
        "a1353832da53723b2fa8692cea211e3cb6bada080ed57c4b3fe4cddb820832fe"
    );
}

#[test]
#[cfg(feature = "web3")]
fn tinywallet_registry_matches_the_verified_v020_release_manifest() {
    let wallet = find("tinywallet").expect("compiled TinyWallet module");
    assert_eq!(wallet.version, "0.20.0");
    #[cfg(feature = "web3")]
    assert_eq!(tinywallet_bus::CONTRACT_VERSION, (1, 14));
    assert_eq!(
        wallet.release_url,
        "https://github.com/tinyhumansai/tinywallet/releases/tag/v0.20.0"
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
            "tinywallet-module-0.20.0-archlinux-arm64.tar.gz",
            "04641e9a3e69a28d7b29b7055cd6b6f1e710f117e37392426a22eb1867e1a0c8",
        ),
        (
            "archlinux-x86_64",
            "tinywallet-module-0.20.0-archlinux-x86_64.tar.gz",
            "50dbe4f6b296941971712eacc831a9ff5947dfaca3be664687e6a1ae120f9b9d",
        ),
        (
            "fedora-43-arm64",
            "tinywallet-module-0.20.0-fedora-43-arm64.tar.gz",
            "3802fd3681bfeac1ce90e34c98600258993cc832fa89285895aabea22f0d2b81",
        ),
        (
            "fedora-43-x86_64",
            "tinywallet-module-0.20.0-fedora-43-x86_64.tar.gz",
            "9767aa07631a03adaed05430c03726f4e19e16b3561f1fc149264eb52332dbc5",
        ),
        (
            "fedora-44-arm64",
            "tinywallet-module-0.20.0-fedora-44-arm64.tar.gz",
            "3974790d265b775f968e5dcaa7e4206d08da52187cad09e438cc278aba9eb540",
        ),
        (
            "fedora-44-x86_64",
            "tinywallet-module-0.20.0-fedora-44-x86_64.tar.gz",
            "83ff87f0d71985e63ac73c95944e534286fde1a04b0a7daa499753e1fe943269",
        ),
        (
            "macos-15-arm64",
            "tinywallet-module-0.20.0-macos-15-arm64.tar.gz",
            "496e61f6ce4a431ee77bf730a713c3d1c8d76b6666f99d15972f7f62623e632f",
        ),
        (
            "macos-15-x86_64",
            "tinywallet-module-0.20.0-macos-15-x86_64.tar.gz",
            "427b9882eb1ef52be3ba28ed9e292790c60a1f769148b3bbafe4ef5389e1144a",
        ),
        (
            "macos-26-arm64",
            "tinywallet-module-0.20.0-macos-26-arm64.tar.gz",
            "65955f1a0c921da47e1a2cecc7ba4f19cd775c536f7039878dd6c60c3804b017",
        ),
        (
            "macos-26-x86_64",
            "tinywallet-module-0.20.0-macos-26-x86_64.tar.gz",
            "5fa92699fed3cb00c57f29c04fa101d94b24f85aedae91f1b6776d57f7162604",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinywallet-module-0.20.0-ubuntu-22.04-arm64.tar.gz",
            "cce338941314005f54584e567dc5d6c2f9a8a4189c4b47908ead8ceed19ae328",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinywallet-module-0.20.0-ubuntu-22.04-x86_64.tar.gz",
            "8712d00897266d0e2b0889ac76fe4da12d1917ffb8e2b3fcc8eb95c1c6f8570e",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinywallet-module-0.20.0-ubuntu-24.04-arm64.tar.gz",
            "1ca76d32b4d66c68b31963f4aa9cfdc6585276fff660d4ab29726dbb29e04499",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinywallet-module-0.20.0-ubuntu-24.04-x86_64.tar.gz",
            "6c092e41f8053851c5d4919e3c8ff6445cac4a9f8526889aa40a500a77ae40ba",
        ),
        (
            "windows-11-arm64",
            "tinywallet-module-0.20.0-windows-11-arm64.zip",
            "171172d90020470dfbe83d3be480b0d6127b123694fdbf600218acc7cba6f929",
        ),
        (
            "windows-2022-x86_64",
            "tinywallet-module-0.20.0-windows-2022-x86_64.zip",
            "80d5ae3272ae0d32fcc1fcb727573554dffcc34f793d35c9b2603d0a4f8cdb28",
        ),
        (
            "windows-2025-x86_64",
            "tinywallet-module-0.20.0-windows-2025-x86_64.zip",
            "c56914add49d2529dc4620e1c1a4846961d16da9b4d8fb9005c3d6d23989d51f",
        ),
    ];
    actual.sort_unstable();
    published.sort_unstable();
    assert_eq!(actual, published);
}

#[test]
fn tinybox_registry_matches_the_published_v0126_release_manifest() {
    let record = find("tinybox").expect("compiled TinyBox module");
    assert_eq!(record.version, "0.1.26");
    assert_eq!(tinybox_bus::CONTRACT_VERSION, (1, 17));
    assert_eq!(
        record.release_url,
        "https://github.com/tinyhumansai/tinybox/releases/tag/v0.1.26"
    );

    let actual = record
        .assets
        .iter()
        .map(|asset| (asset.host_key, asset.archive, asset.sha256))
        .collect::<Vec<_>>();
    let published = [
        (
            "archlinux-arm64",
            "tinybox-0.1.26-archlinux-arm64.tar.gz",
            "7f6984d64be252ae4367d188fe08884154301c7a57b93b830189b345aa3f7ac3",
        ),
        (
            "archlinux-x86_64",
            "tinybox-0.1.26-archlinux-x86_64.tar.gz",
            "35cbc62fb2e24cab231f705bc689954caf11c73352a3eafc51ba810f507687e3",
        ),
        (
            "fedora-43-arm64",
            "tinybox-0.1.26-fedora-43-arm64.tar.gz",
            "14112e7ed9d6dd15543ec68e1bfaa212ca0ea2e62a1dd423d409b4ba3087059b",
        ),
        (
            "fedora-43-x86_64",
            "tinybox-0.1.26-fedora-43-x86_64.tar.gz",
            "07661b6c59c4f6e40c36a25efe1d99fd1a465291ba4b13d58160b3baafc0762f",
        ),
        (
            "fedora-44-arm64",
            "tinybox-0.1.26-fedora-44-arm64.tar.gz",
            "1d1dc263ae85337203b584b4d66b6736ea5bbbe50dfcf0ebd9f83d770e4eceac",
        ),
        (
            "fedora-44-x86_64",
            "tinybox-0.1.26-fedora-44-x86_64.tar.gz",
            "080d50a2527169fbfc4b1cae61dc55468320b1d5d3819db20f05dca29edf3704",
        ),
        (
            "macos-15-arm64",
            "tinybox-0.1.26-macos-15-arm64.tar.gz",
            "806ef4507a1317c295f08356827d53212a627e329c47f0b41db2fbadd1b10f3e",
        ),
        (
            "macos-15-x86_64",
            "tinybox-0.1.26-macos-15-x86_64.tar.gz",
            "3a80345165eda9a2ddd7a09e5447c8042eacee2109a2a7f2e7311efd24a45415",
        ),
        (
            "macos-26-arm64",
            "tinybox-0.1.26-macos-26-arm64.tar.gz",
            "4c63c88eab009946a05e505c146a879ee8446e94103956721af8fa8ecff30c11",
        ),
        (
            "macos-26-x86_64",
            "tinybox-0.1.26-macos-26-x86_64.tar.gz",
            "38ec8ee7bf72783e8c3dbe452664312e1b9f55123a5585132fc1e1e1f19a7f90",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinybox-0.1.26-ubuntu-22.04-arm64.tar.gz",
            "13f9bcdade1873c8cbc82c95e150f06b82182071649906e798bb8d8cd001b9b1",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinybox-0.1.26-ubuntu-22.04-x86_64.tar.gz",
            "ec051ccaf104cd547c1bd4b6248e8ae09d73ea9f908e8d9a7b696d3238aedddb",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinybox-0.1.26-ubuntu-24.04-arm64.tar.gz",
            "4057741f50158040f0d2c7c030c15ae81dbb625d03c0a083c7571e4d0a698d52",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinybox-0.1.26-ubuntu-24.04-x86_64.tar.gz",
            "ec379c022516f6ec1af6288302aed9344a73b64f95b7be45b7e3e64d974552ec",
        ),
        (
            "windows-11-arm64",
            "tinybox-0.1.26-windows-11-arm64.zip",
            "342063b0050d3cfd2f6f957e8f0c407d74d6cda0faebcd145832e7ae3221c820",
        ),
        (
            "windows-2022-x86_64",
            "tinybox-0.1.26-windows-2022-x86_64.zip",
            "74b480d762323e8aa63227d7e4f50f7d05783e440838adbd288d4663a420567e",
        ),
        (
            "windows-2025-x86_64",
            "tinybox-0.1.26-windows-2025-x86_64.zip",
            "f25e8c639c2997fc752bd40840418076a0e86687bd53e746170c05cad5f60abd",
        ),
    ];

    assert_eq!(actual.len(), 17);
    assert_eq!(actual, published);
}
#[test]
fn tinymcp_registry_matches_the_published_v0161_release_manifest() {
    let record = find("tinymcp").expect("compiled TinyMCP module");
    assert_eq!(record.version, "0.16.1");
    assert_eq!(tinymcp_bus::CONTRACT_VERSION, (1, 23));
    assert_eq!(
        record.release_url,
        "https://github.com/tinyhumansai/tinymcp/releases/tag/v0.16.1"
    );
    let actual = record
        .assets
        .iter()
        .map(|asset| (asset.host_key, asset.archive, asset.sha256))
        .collect::<Vec<_>>();
    let published = [
        (
            "archlinux-rolling-arm64",
            "tinymcp-0.16.1-archlinux-arm64.tar.gz",
            "6a1c3f73c4b3002c7ca35a81170a6e727fc0df942607b5b28a65d1f8afa31565",
        ),
        (
            "archlinux-rolling-x86_64",
            "tinymcp-0.16.1-archlinux-x86_64.tar.gz",
            "f6eba52cb67f95c0f21217c02efedc0adc1611204b8e0b49319d9d4b60e53681",
        ),
        (
            "fedora-43-arm64",
            "tinymcp-0.16.1-fedora-43-arm64.tar.gz",
            "34d21f069d145188f0a5a14169f58c71227bca985bacde9062dda8ba8ba8e526",
        ),
        (
            "fedora-43-x86_64",
            "tinymcp-0.16.1-fedora-43-x86_64.tar.gz",
            "7b81c3a28db19f54fec6a98b3226e7a96328633b5bf7de398ef082a551832fbf",
        ),
        (
            "fedora-44-arm64",
            "tinymcp-0.16.1-fedora-44-arm64.tar.gz",
            "e95f24918ea02b355fb269922acc54bb47c49ee31c74f9686903482880851040",
        ),
        (
            "fedora-44-x86_64",
            "tinymcp-0.16.1-fedora-44-x86_64.tar.gz",
            "ee4a4a2f071d951baeb839d3f05573575585af253645e6735edb1aab0f07cd5d",
        ),
        (
            "macos-15-arm64",
            "tinymcp-0.16.1-macos-15-arm64.tar.gz",
            "2aea627b5cf3d5f1d7328dc5ee9b5eb1a67ebd918f6d1dd71987f315b2b9cf47",
        ),
        (
            "macos-15-x86_64",
            "tinymcp-0.16.1-macos-15-x86_64.tar.gz",
            "5aa351af862dc496b6fecb1e355f8fe209959c8148a0c4625341e22ffaaee7d9",
        ),
        (
            "macos-26-arm64",
            "tinymcp-0.16.1-macos-26-arm64.tar.gz",
            "2a7bb7dc867a2f4d033f8c41e9169dad0eee4887b7668ef87df08691021dca5e",
        ),
        (
            "macos-26-x86_64",
            "tinymcp-0.16.1-macos-26-x86_64.tar.gz",
            "03384b0f0b57410d4de9ab121ac3b3267f7af2ead334c7705b4c7b751285fdd9",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinymcp-0.16.1-ubuntu-22.04-arm64.tar.gz",
            "5226c7b9abf1e11ae2070c6d5c57b804264df2b11b1a317a51c40a18da8189be",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinymcp-0.16.1-ubuntu-22.04-x86_64.tar.gz",
            "6c8defda965dfdfb764c6ba7b239cc4656ce98707bda30433d1f77d0b2750087",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinymcp-0.16.1-ubuntu-24.04-arm64.tar.gz",
            "3df2d754b894f37c54a451b988d7d85242f18a92ea02da1388e38b835c739f3d",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinymcp-0.16.1-ubuntu-24.04-x86_64.tar.gz",
            "20e98ee6a52bbcbd53714642400a65285f6039cdad89ce64d04a5b7f3deb168c",
        ),
        (
            "windows-11-arm64",
            "tinymcp-0.16.1-windows-11-arm64.zip",
            "dd81b22ada15f34e3d710e5b7cc3c7ad7e5784d1cb7d784760c72ba77f6950f2",
        ),
        (
            "windows-2022-x86_64",
            "tinymcp-0.16.1-windows-2022-x86_64.zip",
            "21e1422ca50d4206102d406594e78f54974ef39b401d429b6b41d8bcfc8f901b",
        ),
        (
            "windows-2025-x86_64",
            "tinymcp-0.16.1-windows-2025-x86_64.zip",
            "e2ed40815c76717d4c01c2b3b5ae05808d7a0c22402b980f0f6bddf1a50be5d3",
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
    assert_eq!(record.version, "0.1.30");
    assert_eq!(tinychannels_bus::CONTRACT_VERSION, 21);
    assert_eq!(
        tinychannels_bus::module_relay::MODULE_OPERATION_VERSION,
        (1, 21)
    );
    let expected = [
        (
            "macos-15-arm64",
            "tinychannels-module-0.1.30-macos-15-arm64.tar.gz",
            "c23e3c099f33121e14a520dfec994e7b5da6205da02ea001d71b28889e5f269b",
        ),
        (
            "macos-15-x86_64",
            "tinychannels-module-0.1.30-macos-15-x86_64.tar.gz",
            "455e33ffb1d93b0a1023e8b4d528b49ccbc0ed95851b1fb2733649994efe9bfc",
        ),
        (
            "macos-26-arm64",
            "tinychannels-module-0.1.30-macos-26-arm64.tar.gz",
            "b2bcc666b980eab9a3090312ed83482a34591f1f4ecc73346f8e1e2422f482eb",
        ),
        (
            "macos-26-x86_64",
            "tinychannels-module-0.1.30-macos-26-x86_64.tar.gz",
            "d92cb8757f49ebaf3b5d7ecbd79af193a4d4bb002e9b78ce473091d649b08b31",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinychannels-module-0.1.30-ubuntu-22.04-arm64.tar.gz",
            "5852f6888541332cacaed2401ed67815588ba076d60926fdb31daedb13185b12",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinychannels-module-0.1.30-ubuntu-22.04-x86_64.tar.gz",
            "f63bb9034aa48a2335bea0a86ad0f1f920121281b88611c11a9e09fdac8cfe02",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinychannels-module-0.1.30-ubuntu-24.04-arm64.tar.gz",
            "810f68f723e9817d7343944137a00689ef997f57e4e241f496120350a1ba58f6",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinychannels-module-0.1.30-ubuntu-24.04-x86_64.tar.gz",
            "61e7a6f956b17ae53e83a2b0068f1009b0fde0b1266ca4f8f1a7feb8fb077104",
        ),
        (
            "windows-11-arm64",
            "tinychannels-module-0.1.30-windows-11-arm64.zip",
            "122b253a859f769a6ae5e89ea9adc5edaefc5471bf26e6bf8d09e7e0bef3d236",
        ),
        (
            "windows-2022-x86_64",
            "tinychannels-module-0.1.30-windows-2022-x86_64.zip",
            "2a8905d6a72bdbc7c4839fa7afb38362deef54eb2296f918b6af016dd8d54090",
        ),
        (
            "windows-2025-x86_64",
            "tinychannels-module-0.1.30-windows-2025-x86_64.zip",
            "feeddcb7beaa48e4bfda78e40cf6ce999cb7e4abd0414a6595da44665546af88",
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
    assert_eq!(record.version, "0.3.0");
    assert_eq!(tinyvoice_bus::CONTRACT_VERSION, (1, 10));
    let expected = [
        (
            "macos-15-arm64",
            "tinyvoice-module-0.3.0-macos-15-arm64.tar.gz",
            "91b231f6b6e48e3f78a9ae8b3fecab606c7d20f004fe300c030b883df845fd82",
        ),
        (
            "macos-15-x86_64",
            "tinyvoice-module-0.3.0-macos-15-x86_64.tar.gz",
            "f9f4ed3fa6e3615c7f747a619b8c32a2ee252d51541226e27142422273104dad",
        ),
        (
            "macos-26-arm64",
            "tinyvoice-module-0.3.0-macos-26-arm64.tar.gz",
            "4cdbab216808163d8fe33bf9c55eea1355fe1d84cd265167971dac2763f65807",
        ),
        (
            "macos-26-x86_64",
            "tinyvoice-module-0.3.0-macos-26-x86_64.tar.gz",
            "8f12b00fa3474cd5ba2baef3e0e8f8dfb30a401f103bf70d339d8d997ce79597",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinyvoice-module-0.3.0-ubuntu-22.04-arm64.tar.gz",
            "d20f000a9343167016fbfef7007ee6fdd8b84f022cdc15d96c203a0b43aab058",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinyvoice-module-0.3.0-ubuntu-22.04-x86_64.tar.gz",
            "5e1f4b18a6662244fc1488166db153f9df60c651828e44e8bcfd55dc46698087",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinyvoice-module-0.3.0-ubuntu-24.04-arm64.tar.gz",
            "fadbe5b4b9d47744a21096838c10e2ad03b3be43f690f8cef7deb1d2fc1bc427",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinyvoice-module-0.3.0-ubuntu-24.04-x86_64.tar.gz",
            "945fdd762be37c779bdf16b96da03109ce82c7b43d5bb9865e0dfc32e239d883",
        ),
        (
            "windows-11-arm64",
            "tinyvoice-module-0.3.0-windows-11-arm64.zip",
            "7b7e247fb2b6f8be6948ffbe21ed9244e01dac6f367db12b5a78d014d13b8ae9",
        ),
        (
            "windows-2022-x86_64",
            "tinyvoice-module-0.3.0-windows-2022-x86_64.zip",
            "b2485d846a3287c47da303237680e0cbe76a210e5ef2f25027220cde4ffb8ed4",
        ),
        (
            "windows-2025-x86_64",
            "tinyvoice-module-0.3.0-windows-2025-x86_64.zip",
            "ff91872dee9ac3ab098bea1830fe1bebd6f16a8cc3267e02ee36a74d0bc48469",
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
    assert_eq!(search.version, "0.5.0");
    assert_eq!(tinysearch_bus::CONTRACT_VERSION, (2, 2));
    assert_eq!(
        search.asset_for("ubuntu-24.04-x86_64").unwrap().sha256,
        "e038b2529e40d834f4754f3566aaafc5cb5c4a2f0b643a389b4b66ba7f91db74"
    );
}
