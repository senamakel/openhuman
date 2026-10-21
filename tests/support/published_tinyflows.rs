//! Digest gate for tests that load the published TinyFlows v0.8.29 module.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const V0_8_29_MODULE_HASHES: &[&str] = &[
    "04c3d8b0657719f9a5713bbe9bbad4554cd7e470a8c45e9119a60ab97c91c747",
    "9e817d9d956f62e287bf049eb37ec538d54128e27ed2679d2e61059a6fd4050e",
    "978d81646090706ef9b8007e024680b1c4f955e4f13d96546e7d2230bf1766dd",
    "99638f3f299bb49c727b397c9420b937872e76026817630cfd8e182cbe504dcb",
    "5dbe57f3cc3f2292eabaa085d8e4e33725f1071b15713ea343cae21eb4f513f3",
    "3a9433c4d5bf955a5041cdcdc6bdc11b77a92b689161b6708f2e12e962fc2342",
    "369378ca0b2cb340d127e04eb21e53d84dfc2f9248124a0e01b6285a9a208e54",
    "bec7f05ac03851dfef4ae21147f3cb296f86e14f2907b1d1353451dbcd61414b",
    "bd9de15b4072ba94acb120a607e0bdf5b50c93cb6769577e16bd826df4bbd3a0",
    "58ca60ae95de633db68937e797284d908c1b36c7188e9c1fa39b86badf14d213",
    "a0b3afdfb33745bae2a91cddb59ff4f02ceb6ed8157a4ea803350181b7ea3bdf",
];

/// Finds the staged platform artifact and confirms it matches a published v0.8.29 bundle.
pub fn staged_v0_8_29_module() -> (PathBuf, String) {
    let artifact = openhuman_core::modules::local_artifact("tinyflows")
        .expect("stage the published TinyFlows v0.8.29 module on OPENHUMAN_MODULE_PATH");
    let digest = verify_v0_8_29_module(&artifact);
    (artifact, digest)
}

/// Confirms `artifact` is one of the public platform bundles for v0.8.29.
pub fn verify_v0_8_29_module(artifact: &Path) -> String {
    let digest = hex::encode(Sha256::digest(
        std::fs::read(artifact).expect("read staged TinyFlows native artifact"),
    ));
    assert!(
        V0_8_29_MODULE_HASHES.contains(&digest.as_str()),
        "staged TinyFlows library is not a published v0.8.29 module: {} sha256={digest}",
        artifact.display()
    );
    digest
}
