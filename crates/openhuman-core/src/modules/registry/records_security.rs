//! TinySecurity admission record. Release assets must be copied from its first
//! published checksum manifest before the dependent migration can ship.
use crate::modules::types::{LoadPolicy, ModuleRecord};

pub(crate) const TINYSECURITY: ModuleRecord = ModuleRecord {
    id: "tinysecurity",
    description: "Native security policy and immutable filesystem authorization scopes",
    bus_name: tinysecurity_bus::names::INTERFACE,
    object_path: tinysecurity_bus::names::OBJECT_PATH,
    version: "0.2.1",
    // No release exists yet. An empty release source and asset set deliberately
    // prevent downloads and trusted calls; local artifacts cannot fill this gap.
    release_url: "",
    assets: &[],
    load: LoadPolicy::Eager,
};
