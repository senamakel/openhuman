use super::{find, ALL};
use tinybus::module::platform::candidates_for;

#[path = "registry_computer_release_tests.rs"]
mod computer_release_tests;

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
fn tinybox_registry_matches_the_published_v0140_release_manifest() {
    let record = find("tinybox").expect("compiled TinyBox module");
    assert_eq!(record.version, "0.1.40");
    assert_eq!(tinybox_bus::CONTRACT_VERSION, (1, 22));
    assert_eq!(
        record.release_url,
        "https://github.com/tinyhumansai/tinybox/releases/tag/v0.1.40"
    );

    let actual = record
        .assets
        .iter()
        .map(|asset| (asset.host_key, asset.archive, asset.sha256))
        .collect::<Vec<_>>();
    let published = [
        (
            "archlinux-arm64",
            "tinybox-0.1.40-archlinux-arm64.tar.gz",
            "303883247b457c47d787efe77fdf9378d32a7752b5891bb8f8590c8919c9310b",
        ),
        (
            "archlinux-x86_64",
            "tinybox-0.1.40-archlinux-x86_64.tar.gz",
            "4b2aeb9fc91c4ed6fd8d27fb1ee6666a90cfde31108dc8012073fe4b0d1bf8b1",
        ),
        (
            "fedora-43-arm64",
            "tinybox-0.1.40-fedora-43-arm64.tar.gz",
            "c13130d96a159b29316f0c41f63c25b3b0de23b0476f7e988219f121c28c96ee",
        ),
        (
            "fedora-43-x86_64",
            "tinybox-0.1.40-fedora-43-x86_64.tar.gz",
            "1685d8efd5267a56563f6f3e1e3fe3f7d43ec85d29b218dc65ae923df45e826d",
        ),
        (
            "fedora-44-arm64",
            "tinybox-0.1.40-fedora-44-arm64.tar.gz",
            "0c03e21248ba303baf6ee83410b77fd366bea0f66028d2fb54983e1ef2a4fbfa",
        ),
        (
            "fedora-44-x86_64",
            "tinybox-0.1.40-fedora-44-x86_64.tar.gz",
            "4def3f78e3963b33a937cd47c89d8940af8ada9b037b9dc040ad4cf96d81c0dd",
        ),
        (
            "macos-15-arm64",
            "tinybox-0.1.40-macos-15-arm64.tar.gz",
            "aae2e9d0e3df8f8253c5a3ef3a0b9fb1d224e0a4ee689b5bdfecd8ac1f192bec",
        ),
        (
            "macos-15-x86_64",
            "tinybox-0.1.40-macos-15-x86_64.tar.gz",
            "0800d426346160ec84b61a47b8c339321cbfcded8805cdac96ee53f763df0646",
        ),
        (
            "macos-26-arm64",
            "tinybox-0.1.40-macos-26-arm64.tar.gz",
            "078348a5263cc13e58cbdef41c82b12a7b94781f7f15aae84886a428445d301c",
        ),
        (
            "macos-26-x86_64",
            "tinybox-0.1.40-macos-26-x86_64.tar.gz",
            "7240c5c369bd20dc6466ec42d9104bf14d59225fac55feb4eed684885ce3efb9",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinybox-0.1.40-ubuntu-22.04-arm64.tar.gz",
            "456498d248a748200a2372c0dc75a6880789b4beec437916b921e10ab982f751",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinybox-0.1.40-ubuntu-22.04-x86_64.tar.gz",
            "ade2273ca24573831b38a7513aec89e98f7e1bf56ed16759d87e11aa6813a0e9",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinybox-0.1.40-ubuntu-24.04-arm64.tar.gz",
            "4fbc9399be13f95f3d5ce9dea9d966fcab306324e422c081281b0c16f0f7090c",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinybox-0.1.40-ubuntu-24.04-x86_64.tar.gz",
            "16c2aa1d293bc09fe882714f380d605d80537936956e3a61e9987333abe5cbe1",
        ),
        (
            "windows-11-arm64",
            "tinybox-0.1.40-windows-11-arm64.zip",
            "73307f0b6a7a6d6978f6e9ecd9092a957f64e2daa4b03ea3a9170207f5212f37",
        ),
        (
            "windows-2022-x86_64",
            "tinybox-0.1.40-windows-2022-x86_64.zip",
            "688f4184225cf392cdfa00f4cc7ef8ca5e1d2b38bbf4ebc3a1f63ea133b48007",
        ),
        (
            "windows-2025-x86_64",
            "tinybox-0.1.40-windows-2025-x86_64.zip",
            "7ac5140fdde15208a61d52c1bc8d9ba3f610337f6964e64fd53303c4f68bf5cb",
        ),
    ];

    assert_eq!(actual.len(), 17);
    assert_eq!(actual, published);
}
#[test]
fn tinymcp_registry_matches_the_published_v0195_release_manifest() {
    let record = find("tinymcp").expect("compiled TinyMCP module");
    assert_eq!(record.version, "0.19.5");
    assert_eq!(tinymcp_bus::CONTRACT_VERSION, (1, 27));
    assert_eq!(
        record.release_url,
        "https://github.com/tinyhumansai/tinymcp/releases/tag/v0.19.5"
    );
    let actual = record
        .assets
        .iter()
        .map(|asset| (asset.host_key, asset.archive, asset.sha256))
        .collect::<Vec<_>>();
    let published = [
        (
            "archlinux-rolling-arm64",
            "tinymcp-0.19.5-archlinux-arm64.tar.gz",
            "ca2e8bc52523a9e719ec4ac14e5be52ba9389afb57faeaaeb9cbd56000b29528",
        ),
        (
            "archlinux-rolling-x86_64",
            "tinymcp-0.19.5-archlinux-x86_64.tar.gz",
            "33f4aab3a961faeb3fc64ec8055b712d73dc297152eb8002c432a1855d1869c3",
        ),
        (
            "fedora-43-arm64",
            "tinymcp-0.19.5-fedora-43-arm64.tar.gz",
            "fa66fa9092c176e6a37beeb2af157d5acf530b7bf3c7786459e09daf2f8a75ba",
        ),
        (
            "fedora-43-x86_64",
            "tinymcp-0.19.5-fedora-43-x86_64.tar.gz",
            "09965afec03286005bae27e7dd7c29251645f42bdc933b3457190d655e87960c",
        ),
        (
            "fedora-44-arm64",
            "tinymcp-0.19.5-fedora-44-arm64.tar.gz",
            "62be25f9927ac08c03a7638386e0c904e7313d4255a3b85264774165bb3feb6d",
        ),
        (
            "fedora-44-x86_64",
            "tinymcp-0.19.5-fedora-44-x86_64.tar.gz",
            "7e4fd1a3261cbc166ed358cafb93a665d2464197beb2f896d44f0382f07ec2c6",
        ),
        (
            "macos-15-arm64",
            "tinymcp-0.19.5-macos-15-arm64.tar.gz",
            "1e2023a2b6526a9a465916c18d714306dd953d5c887aa0226a2fffd15a77dab0",
        ),
        (
            "macos-15-x86_64",
            "tinymcp-0.19.5-macos-15-x86_64.tar.gz",
            "e3147cb5ab4599f3c308807137d902717677c59f15954fa48b73201a1a3aed15",
        ),
        (
            "macos-26-arm64",
            "tinymcp-0.19.5-macos-26-arm64.tar.gz",
            "0c2df4171521bb80122ad9f3eca01dab34def059023f69f97eac2337cadb2a16",
        ),
        (
            "macos-26-x86_64",
            "tinymcp-0.19.5-macos-26-x86_64.tar.gz",
            "d07df68db79ce77afe03ae387e9304b59fb22bdebc95ef7bf3447326f8b1f3c5",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinymcp-0.19.5-ubuntu-22.04-arm64.tar.gz",
            "c80340656ca14cc75802d8ee896a725a83b589eaba7c967afafb255b731dc785",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinymcp-0.19.5-ubuntu-22.04-x86_64.tar.gz",
            "db59bec7a6e81195b15ac2ab23819da2515ffe0d610896de6a3732cf9111f74e",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinymcp-0.19.5-ubuntu-24.04-arm64.tar.gz",
            "1706005201deb02965c8d5c3abe96bb85bf58c7fb301ecc64ca6541a634568f2",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinymcp-0.19.5-ubuntu-24.04-x86_64.tar.gz",
            "de0caac98c9059fe36338d27eb18d1c11110f68c13e8cd524dd6c5f72efe0d51",
        ),
        (
            "windows-11-arm64",
            "tinymcp-0.19.5-windows-11-arm64.zip",
            "20c059fda61b1f32fb0d957b55b0994708b0f8fb8f604cfe15b652e6ecdb84c3",
        ),
        (
            "windows-2022-x86_64",
            "tinymcp-0.19.5-windows-2022-x86_64.zip",
            "91db87594204b69032184243799fe858439e90035000bad9420b1629b5fe9aa8",
        ),
        (
            "windows-2025-x86_64",
            "tinymcp-0.19.5-windows-2025-x86_64.zip",
            "9898dd468f31ec3d11b14d69b7c3cda1432ba726fcb608fe50da5a3045719916",
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
    assert_eq!(record.version, "0.1.38");
    assert_eq!(tinychannels_bus::CONTRACT_VERSION, 21);
    assert_eq!(
        tinychannels_bus::module_relay::MODULE_OPERATION_VERSION,
        (1, 21)
    );
    let expected = [
        (
            "macos-15-arm64",
            "tinychannels-module-0.1.38-macos-15-arm64.tar.gz",
            "1c092b81c7bde10a5d83f6f797c2dd80b9992b5c292ecee06c0c8fc866c395bd",
        ),
        (
            "macos-15-x86_64",
            "tinychannels-module-0.1.38-macos-15-x86_64.tar.gz",
            "b0302b04b0feb2cdc4ddaf6ede9063eb727db95f4a1f07dc3140c03b8deff546",
        ),
        (
            "macos-26-arm64",
            "tinychannels-module-0.1.38-macos-26-arm64.tar.gz",
            "ba84151bf95ef809703c87fef6c37f598da7c492f659a10ffb9893e0e8209ffd",
        ),
        (
            "macos-26-x86_64",
            "tinychannels-module-0.1.38-macos-26-x86_64.tar.gz",
            "421de74c0fb993ccfac3a927d4ac192d615716358f5ed72cd3ce0ab17e904ee4",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinychannels-module-0.1.38-ubuntu-22.04-arm64.tar.gz",
            "ec92e0622f2b10f6c102c5b2e808695d500eacf75c7d99c558407b085e6cee4d",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinychannels-module-0.1.38-ubuntu-22.04-x86_64.tar.gz",
            "f300153ac2cb857896550bf5e53919426f43eee684f259e5d7f761b22054f1c2",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinychannels-module-0.1.38-ubuntu-24.04-arm64.tar.gz",
            "a8e4cc8cfd5528e07ffe5e6c9963d21bfd3b124bbedab7ac6c2ecbfc3e8a2821",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinychannels-module-0.1.38-ubuntu-24.04-x86_64.tar.gz",
            "7a30f6fb0f71ef6627b246e91040a7938f94aa0519bfda93a043709aa0141e49",
        ),
        (
            "windows-11-arm64",
            "tinychannels-module-0.1.38-windows-11-arm64.zip",
            "26f919a77fefd8d65c7eb6a2231a35059cb2049b3b7877673073558e19d600dd",
        ),
        (
            "windows-2022-x86_64",
            "tinychannels-module-0.1.38-windows-2022-x86_64.zip",
            "ffeec695e7aad3c22dc885266badc08cf330c885425dacdbe5f35cd9ad72bfc3",
        ),
        (
            "windows-2025-x86_64",
            "tinychannels-module-0.1.38-windows-2025-x86_64.zip",
            "5ed7a3875d6e103fdd5a577e078e7288d0dfa7eb9c4b380ab4fe0e4246611aa9",
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
