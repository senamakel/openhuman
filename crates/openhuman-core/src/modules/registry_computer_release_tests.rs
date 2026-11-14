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
            "d1f4b1d3876e54be89450e87d595d46fdc2c7fac2bb6041fe1b68f0c784f94ae",
        ),
        (
            "archlinux-x86_64",
            "tinycomputer-0.16.1-archlinux-x86_64.tar.gz",
            "e6182fa507e3b9d49dbd77b007959319277f1ffcdb3febfd2fb43db213418a0a",
        ),
        (
            "fedora-43-arm64",
            "tinycomputer-0.16.1-fedora-43-arm64.tar.gz",
            "e09f1739856034cd85f1024b122267aed25901a1bcfa4dcb7e9bc8ae09958e06",
        ),
        (
            "fedora-43-x86_64",
            "tinycomputer-0.16.1-fedora-43-x86_64.tar.gz",
            "e7ae93eec1ce839f40a5e3ecd0feb8357c2ec1821f3450a1eae61968cff99910",
        ),
        (
            "fedora-44-arm64",
            "tinycomputer-0.16.1-fedora-44-arm64.tar.gz",
            "e123801af99f62e797bb68fc9343aad93bd006834b9cd0d820a3457fc2eca066",
        ),
        (
            "fedora-44-x86_64",
            "tinycomputer-0.16.1-fedora-44-x86_64.tar.gz",
            "c34bc5084bbf90182f6e6907397476e3a3062d155a2b5af0ded65834c47ac636",
        ),
        (
            "macos-15-arm64",
            "tinycomputer-0.16.1-macos-15-arm64.tar.gz",
            "dcf06a95dc7ac238b4ddbf2a9c91b20775f525802ca951b6dd7cf66f686c16bb",
        ),
        (
            "macos-15-x86_64",
            "tinycomputer-0.16.1-macos-15-x86_64.tar.gz",
            "45b7bc48557a08e055b54f073e079634774833885c8f3d8807a5602b3785776f",
        ),
        (
            "macos-26-arm64",
            "tinycomputer-0.16.1-macos-26-arm64.tar.gz",
            "1d63287a36bc38d009d8cd1bc04dea8824cc7f9fd8b323717ee319ff95a7f96e",
        ),
        (
            "macos-26-x86_64",
            "tinycomputer-0.16.1-macos-26-x86_64.tar.gz",
            "d7360c57c6101fc11bc72d8ad1be3f5c940bc65cb386f8ca05bb9cb9c2f384eb",
        ),
        (
            "ubuntu-22.04-arm64",
            "tinycomputer-0.16.1-ubuntu-22.04-arm64.tar.gz",
            "151e633b9b3fcc54fd088612afbb0ebc5e2cb1ebc2951f894502458788ccefeb",
        ),
        (
            "ubuntu-22.04-x86_64",
            "tinycomputer-0.16.1-ubuntu-22.04-x86_64.tar.gz",
            "ec9f749010f351e2d97e9688bd52705365353410bd92a477b7650749a2fd15ae",
        ),
        (
            "ubuntu-24.04-arm64",
            "tinycomputer-0.16.1-ubuntu-24.04-arm64.tar.gz",
            "37f5d161c00e8ffcd32d3a96ddc462b1459f5eceaa586697c23a59d401c5e5be",
        ),
        (
            "ubuntu-24.04-x86_64",
            "tinycomputer-0.16.1-ubuntu-24.04-x86_64.tar.gz",
            "60ca5872e3c26caa3dfce15d83b65ab3520be58d9ee330983068251def92ca9e",
        ),
        (
            "windows-11-arm64",
            "tinycomputer-0.16.1-windows-11-arm64.zip",
            "911cb39d17034c3f903a112962967e4968802b7bee87dd0561926e8b66b7906b",
        ),
        (
            "windows-2022-x86_64",
            "tinycomputer-0.16.1-windows-2022-x86_64.zip",
            "b9b42f9290339e15b8906d4912e19177c66c468610e5191e0d2f8533f891fa73",
        ),
        (
            "windows-2025-x86_64",
            "tinycomputer-0.16.1-windows-2025-x86_64.zip",
            "7bdb2be087da72d49cb3c26a89bee7b3e76f075fdc475f3186eba9c2da575049",
        ),
    ];
    assert_eq!(desktop.assets.len(), published.len());
    actual.sort_unstable();
    published.sort_unstable();
    assert_eq!(actual, published);
}
