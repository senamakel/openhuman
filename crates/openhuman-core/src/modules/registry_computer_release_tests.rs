use super::*;

#[test]
fn tinycomputer_registry_matches_bus_contract_and_published_release() {
    let desktop = find("tinycomputer").expect("compiled computer module");
    assert_eq!(desktop.bus_name, tinycomputer_bus::names::INTERFACE);
    assert_eq!(desktop.object_path, tinycomputer_bus::names::OBJECT_PATH);
    assert_eq!(desktop.version, "0.16.1");
    assert_eq!(tinycomputer_bus::CONTRACT_VERSION, (3, 1));

    let mut actual = desktop
        .assets
        .iter()
        .map(|asset| (asset.host_key, asset.archive, asset.sha256))
        .collect::<Vec<_>>();
    let mut published = [
        (
            "archlinux-arm64",
            "tinycomputer-0.16.1-archlinux-arm64.tar.gz",
            "61e31f69269370222513e814a8214c206a6f6e1eecc082ad1849a3ea06332b36",
        ),
        (
            "archlinux-x86_64",
            "tinycomputer-0.16.1-archlinux-x86_64.tar.gz",
            "3f16a02e1b514bbc1cd253194c255e31434887914234e25905b71da56c0fa32a",
        ),
        (
            "fedora-43-arm64",
            "tinycomputer-0.16.1-fedora-43-arm64.tar.gz",
            "17e05040028c54b24635e3b15a40e8d8b991e2e761adfe4653d520334dd2b563",
        ),
        (
            "fedora-43-x86_64",
            "tinycomputer-0.16.1-fedora-43-x86_64.tar.gz",
            "ff83310c6d096a3fc242daa22e5de793e2ebf3aededb23c19118c983653bdc0d",
        ),
        (
            "fedora-44-arm64",
            "tinycomputer-0.16.1-fedora-44-arm64.tar.gz",
            "f23ae2c2f707238a4da065753774c09fa086884dd5aaa4457c2dfd63cfed8aef",
        ),
        (
            "fedora-44-x86_64",
            "tinycomputer-0.16.1-fedora-44-x86_64.tar.gz",
            "efeabbfab0752d3d298c7de100e5e3e04e9aef20edf77fa5f9af39325e02223e",
        ),
        (
            "macos-15-arm64",
            "tinycomputer-0.16.1-macos-15-arm64.tar.gz",
            "b2c65bb60e8920aa635c008a12a79b1c5dbc31095f4692a12ac7abe9015658ff",
        ),
        (
            "macos-15-x86_64",
            "tinycomputer-0.16.1-macos-15-x86_64.tar.gz",
            "7dde8666a3c434f0c026e1cf2bcbeb638ad32350487c265e5860277b894c6426",
        ),
        (
            "macos-26-arm64",
            "tinycomputer-0.16.1-macos-26-arm64.tar.gz",
            "64e73fab406cd9c2171186e12b96a73b2ec5ad0e6a128909018461160ce65a0e",
        ),
        (
            "macos-26-x86_64",
            "tinycomputer-0.16.1-macos-26-x86_64.tar.gz",
            "c308d3db1c9be71117ea28ff9d28cebfb41e9fa75f6be8c4c7bb0cfe17f3aeed",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinycomputer-0.16.1-ubuntu-22.04-arm64.tar.gz",
            "43c40bff8cf7a7f5a058a96f28895a628d54bc1a9d62e9a354a0e86f52eff6a0",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinycomputer-0.16.1-ubuntu-22.04-x86_64.tar.gz",
            "625c3bd95d9f170e8794d3bf54e1dc4c2e2356c921e8277c65e45efe92fa1cc6",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinycomputer-0.16.1-ubuntu-24.04-arm64.tar.gz",
            "67a7d741cc17c3e59b4021ce78505b9f99b93ca92b8b247bcf650c152c5e7652",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinycomputer-0.16.1-ubuntu-24.04-x86_64.tar.gz",
            "d2394934e358040dd27869da3527d79fca45bfa14721dc7c0d4b345ee3c6a78b",
        ),
        (
            "windows-11-arm64",
            "tinycomputer-0.16.1-windows-11-arm64.zip",
            "1d48f1efddf3b32ced18e5fe8d09e3d564dedca8f8ee89c20a2c8ffdb08f2035",
        ),
        (
            "windows-2022-x86_64",
            "tinycomputer-0.16.1-windows-2022-x86_64.zip",
            "819c7d3911e97e615336add99a8504ad47dc40f26cce33085d7c43b1f985a4f2",
        ),
        (
            "windows-2025-x86_64",
            "tinycomputer-0.16.1-windows-2025-x86_64.zip",
            "9d970bfcefacac52603d7ee3fe3fc8e00bf962ed8be291ebb9c66b7d0696f8f4",
        ),
    ];
    assert_eq!(desktop.assets.len(), published.len());
    actual.sort_unstable();
    published.sort_unstable();
    assert_eq!(actual, published);
}
