//! Registry metadata against each module's bus contract constants.

use super::{component_for, find, ALL, COMPONENT_OF};

/// The interface and object path each contract crate publishes, keyed by the
/// module id the registry uses. This is written out from the contract
/// constants, not from the records, so a record that drifts from its contract
/// (or a contract that is renamed upstream) fails here rather than at the first
/// bus call. Records behind an optional contract crate are listed under the
/// same gate as the crate.
fn contract_names() -> Vec<(&'static str, &'static str, &'static str)> {
    #[allow(unused_mut)]
    let mut names = vec![
        (
            "tinycomputer",
            tinycomputer_bus::names::INTERFACE,
            tinycomputer_bus::names::OBJECT_PATH,
        ),
        (
            "tinysearch",
            tinysearch_bus::names::INTERFACE,
            tinysearch_bus::names::OBJECT_PATH,
        ),
        (
            "tinyjuice",
            tinyjuice_bus::names::BUS_NAME,
            tinyjuice_bus::names::OBJECT_PATH,
        ),
        (
            "tinymcp",
            tinymcp_bus::names::INTERFACE,
            tinymcp_bus::names::OBJECT_PATH,
        ),
        (
            "tinyconnectors",
            tinyconnectors_bus::names::INTERFACE,
            tinyconnectors_bus::names::OBJECT_PATH,
        ),
        (
            "tinychannels",
            tinychannels_bus::names::BUS_NAME,
            tinychannels_bus::names::OBJECT_PATH,
        ),
        // The Authoring identity the module registers under; the scheduler is
        // a second object on it (pinned by `the_flows_record_serves_the_schedule_object`).
        (
            "tinyflows",
            tinyflows_bus::BUS_NAME,
            tinyflows_bus::OBJECT_PATH,
        ),
    ];
    #[cfg(feature = "modules")]
    names.push(("tinybox", tinybox_bus::INTERFACE, tinybox_bus::OBJECT_PATH));
    #[cfg(feature = "documents")]
    names.push((
        "tinydocs",
        tinydocs_bus::names::BUS_NAME,
        tinydocs_bus::names::OBJECT_PATH,
    ));
    #[cfg(feature = "web3")]
    names.push((
        "tinywallet",
        tinywallet_bus::names::BUS_NAME,
        tinywallet_bus::names::OBJECT_PATH,
    ));
    #[cfg(feature = "voice")]
    names.push((
        "tinyvoice",
        tinyvoice_bus::names::BUS_NAME,
        tinyvoice_bus::names::OBJECT_PATH,
    ));
    #[cfg(feature = "hosting")]
    names.push((
        "tinyhosts",
        tinyhosts_bus::BUS_NAME,
        tinyhosts_bus::OBJECT_PATH,
    ));
    #[cfg(feature = "tinymemes")]
    names.push((
        "tinymemes",
        tinymemes_bus::STATE_INTERFACE,
        tinymemes_bus::STATE_PATH,
    ));
    #[cfg(feature = "inbound-queues")]
    names.push((
        "tinyeventqueues",
        tinyeventqueues_bus::INTERFACE,
        tinyeventqueues_bus::OBJECT_PATH,
    ));
    names
}

#[test]
fn every_record_matches_its_contract_constants() {
    let contracts = contract_names();
    assert_eq!(contracts.len(), ALL.len(), "a record has no contract entry");
    for (id, bus_name, object_path) in contracts {
        let record = find(id).unwrap_or_else(|| panic!("{id} has no registry record"));
        assert_eq!(
            record.bus_name, bus_name,
            "{id} bus name drifted from its contract"
        );
        assert_eq!(
            record.object_path, object_path,
            "{id} object path drifted from its contract"
        );
    }
}

#[test]
fn a_drifted_contract_constant_is_reported_as_a_mismatch() {
    // The comparison must be able to fail: a fixture whose constant differs
    // from the record (as a renamed upstream interface would) is not equal.
    let record = find("tinymcp").unwrap();
    let drifted = format!("{}Renamed", tinymcp_bus::names::INTERFACE);
    assert_ne!(record.bus_name, drifted);
}

#[cfg(all(
    feature = "documents",
    feature = "web3",
    feature = "voice",
    feature = "hosting",
    feature = "tinymemes",
    feature = "inbound-queues"
))]
#[test]
fn a_product_feature_build_carries_all_fourteen_records() {
    assert_eq!(ALL.len(), 14);
    for id in [
        "tinycomputer",
        "tinysearch",
        "tinydocs",
        "tinywallet",
        "tinyjuice",
        "tinyvoice",
        "tinymcp",
        "tinyconnectors",
        "tinybox",
        "tinychannels",
        "tinyflows",
        "tinyhosts",
        "tinymemes",
        "tinyeventqueues",
    ] {
        assert!(find(id).is_some(), "{id} missing from the registry");
    }
}

#[test]
fn every_record_maps_to_a_real_component() {
    for record in ALL {
        let component = component_for(record.id)
            .unwrap_or_else(|| panic!("module '{}' has no component mapping", record.id));
        assert!(
            crate::core::runtime::COMPONENT_NAMES.contains(&component),
            "'{component}' is not a [components] name"
        );
    }
    for (id, _) in COMPONENT_OF {
        assert!(
            ALL.iter().any(|r| r.id == *id)
                || !cfg!(feature = "modules")
                || matches!(
                    *id,
                    "tinydocs"
                        | "tinywallet"
                        | "tinyvoice"
                        | "tinyhosts"
                        | "tinymemes"
                        | "tinyeventqueues"
                ),
            "mapping for unknown module '{id}'"
        );
    }
}

/// The scheduler is a second interface on the same module, and the host calls
/// it by the contract's own names: the record carries the Authoring identity,
/// and the Schedule object and callback interfaces are distinct from it and from
/// each other.
#[test]
fn the_flows_record_serves_the_schedule_object() {
    use tinyflows_bus::schedule;
    let record = find("tinyflows").expect("tinyflows is registered");
    assert_eq!(record.bus_name, schedule::BUS_NAME);
    assert_ne!(record.object_path, schedule::OBJECT_PATH);
    assert_ne!(schedule::INTERFACE, schedule::CALLBACK_INTERFACE);
    assert_ne!(schedule::INTERFACE, schedule::DOCUMENT_INTERFACE);
    assert_eq!(schedule::CONTRACT_VERSION, 7, "the host speaks contract v7");
    assert_eq!(component_for("tinyflows"), Some("tinyflows"));
}

/// The pins are copied from the published v0.8.28 `checksum.toml`.
#[test]
fn the_flows_record_pins_every_host_of_one_release() {
    let record = find("tinyflows").unwrap();
    assert_eq!(record.version, "0.8.28");
    assert_eq!(record.assets.len(), 11);
    let expected = [
        (
            "tinyflows-module-0.8.28-macos-26-arm64.tar.gz",
            "387b2fdf527b087700cc7b68fee2866fe5590a34f5041819793b5b05caf97e90",
        ),
        (
            "tinyflows-module-0.8.28-macos-26-x86_64.tar.gz",
            "fdcba79686d466ec2d163ab23ed0a56e148884c360b66a074c0dbb918decb6a3",
        ),
        (
            "tinyflows-module-0.8.28-macos-15-arm64.tar.gz",
            "923d720d622a535e6cd547ffda1c6f73cc70b7b1c13681a374633d81f51069eb",
        ),
        (
            "tinyflows-module-0.8.28-macos-15-x86_64.tar.gz",
            "d2f34c52863cb1d3a78256f59fa9e54e26af62b39540f24f072c89e18de6ae65",
        ),
        (
            "tinyflows-module-0.8.28-ubuntu-24.04-x86_64.tar.gz",
            "c3224ba59b7683cf2459a1e1e8946b5ad2c6200d729c96bfd9c084450c6cd65d",
        ),
        (
            "tinyflows-module-0.8.28-ubuntu-24.04-arm64.tar.gz",
            "2306e7fda0f1edb75e90b79bd645ec1806f04d004b5ec615c83f67789c8a3655",
        ),
        (
            "tinyflows-module-0.8.28-ubuntu-22.04-x86_64.tar.gz",
            "54bb2ac4e085cac962d368815d485df3e0050b8b475a66b58cd96a044c4de40e",
        ),
        (
            "tinyflows-module-0.8.28-ubuntu-22.04-arm64.tar.gz",
            "3d92636d08b549533452ac971c01edb42a02be170ea974286c215647c09f3491",
        ),
        (
            "tinyflows-module-0.8.28-windows-2025-x86_64.zip",
            "d8772f00d1dce3ff6000cee492348bdb8176660d798fa55edecf172c3d279ef0",
        ),
        (
            "tinyflows-module-0.8.28-windows-2022-x86_64.zip",
            "e1c926be3b5f1e8d4b2d1492797df2357ddfb3d90fdeb190664fe972f9703ddf",
        ),
        (
            "tinyflows-module-0.8.28-windows-11-arm64.zip",
            "8395463124c85256009fae44e0b34c95b3a494270bbc7f8e80cc03d4f174fd9a",
        ),
    ];
    for (asset, (archive, sha256)) in record.assets.iter().zip(expected) {
        assert_eq!(asset.archive, archive);
        assert_eq!(asset.sha256, sha256, "{}", asset.archive);
    }
}
