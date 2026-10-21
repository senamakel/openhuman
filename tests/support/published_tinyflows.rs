//! Digest gate for tests that load the published TinyFlows v0.8.28 module.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const V0_8_28_MODULE_HASHES: &[&str] = &[
    "ef51e8177c24958412213232fdc32180a84052e736d815f96126d05b61a19f38",
    "eb4480c8388ffc93cb42c3333586e562dea39e1ee5f725ab4c8066aba43b9c9b",
    "6886ed89e6b9111756b76856f864413888b5c99298f6eb92726a7b27f1253ea4",
    "5a5a83aa50b7fa2372d9353db19f8aabc4c9b52ea7f9889ff4b8e0b8ef1b451d",
    "63839f388b82c5a896837f052733f1a40a3fe8e8110575f3c39086de9db9f81c",
    "caf6afe351d8a49121c5281534243e8a00a9682f697a255ef30ac28803d2a483",
    "1e767785a82448c5e454d386544a418dd25f42e780adcc306cbcc2bc9fca9ca4",
    "456a6755c0a0faaf162894cfe4d16429f2bdd03cf8ab49073c35923621a49b2e",
    "ef9fabd28ce64d5291b6e8a03a3a81c1ed0e36f9ef2a057827d10ef9d3dba8b1",
    "17b546581cff5e2f227b37b64c4cc9afc37ec85713f74b1b57ae7451b75177e9",
    "df699b2ddf661f23cc4104730dbae824372bca504cd8667d6ea50c0be4d0b4fc",
];

/// Finds the staged platform artifact and confirms it matches a published v0.8.28 bundle.
pub fn staged_v0_8_28_module() -> (PathBuf, String) {
    let artifact = openhuman_core::modules::local_artifact("tinyflows")
        .expect("stage the published TinyFlows v0.8.28 module on OPENHUMAN_MODULE_PATH");
    let digest = verify_v0_8_28_module(&artifact);
    (artifact, digest)
}

/// Confirms `artifact` is one of the public platform bundles for v0.8.28.
pub fn verify_v0_8_28_module(artifact: &Path) -> String {
    let digest = hex::encode(Sha256::digest(
        std::fs::read(artifact).expect("read staged TinyFlows native artifact"),
    ));
    assert!(
        V0_8_28_MODULE_HASHES.contains(&digest.as_str()),
        "staged TinyFlows library is not a published v0.8.28 module: {} sha256={digest}",
        artifact.display()
    );
    digest
}
