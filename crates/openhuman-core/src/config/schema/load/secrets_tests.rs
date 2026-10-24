use super::*;

#[test]
fn gemini_key_round_trips_with_encryption_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.config_path = dir.path().join("config.toml");
    config.secrets.encrypt = true;
    config.search.gemini.api_key = Some("gemini-sentinel".into());

    encrypt_config_secrets(&mut config).unwrap();
    let ciphertext = config.search.gemini.api_key.as_deref().unwrap();
    assert!(ciphertext.starts_with("enc2:"));
    assert!(!ciphertext.contains("gemini-sentinel"));
    assert!(!decrypt_config_secrets(&mut config, dir.path()).unwrap());
    assert_eq!(
        config.search.gemini.api_key.as_deref(),
        Some("gemini-sentinel")
    );
}

#[test]
fn storage_url_round_trips_and_fails_closed_without_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.config_path = dir.path().join("config.toml");
    config.secrets.encrypt = true;
    config.storage.url = Some("mongodb://app:hunter2@db.internal/openhuman".into());

    encrypt_config_secrets(&mut config).unwrap();
    let sealed = config.storage.url.clone().unwrap();
    assert!(sealed.starts_with("enc2:") && !sealed.contains("hunter2"));
    decrypt_config_secrets(&mut config, dir.path()).unwrap();
    assert_eq!(
        config.storage.url.as_deref(),
        Some("mongodb://app:hunter2@db.internal/openhuman")
    );

    // A key that cannot open the URL must not clear it into "no URL".
    let other = tempfile::tempdir().unwrap();
    config.storage.url = Some(sealed.clone());
    decrypt_config_secrets(&mut config, other.path()).unwrap();
    assert_eq!(config.storage.url.as_deref(), Some(sealed.as_str()));
}

/// Every field that holds a user credential is sealed on save and opened on
/// load. A credential field missing from the walk is written to `config.toml`
/// in plaintext.
#[test]
fn every_stored_credential_is_encrypted_at_rest() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.config_path = dir.path().join("config.toml");
    config.secrets.encrypt = true;
    let key = |slot: &str| Some(format!("sentinel-{slot}"));
    config.api_key = key("api_key");
    config.seltz.api_key = key("seltz");
    for p in [
        "brave", "querit", "exa", "tavily", "gemini", "parallel", "tinyfish",
    ] {
        config.search.credentials_mut(p).unwrap().api_key = key(p);
    }
    config.local_ai.api_key = key("local_ai");
    config.hosting.api_key = "sentinel-hosting".into();
    config.composio.api_key = key("composio");
    config.integrations.google_places.api_key = key("google_places");
    config.integrations.parallel.api_key = key("integrations-parallel");
    config.integrations.tinyfish.api_key = key("integrations-tinyfish");
    config.integrations.stock_prices.api_key = key("stock_prices");
    let plain = config.clone();

    encrypt_config_secrets(&mut config).unwrap();
    let at_rest = toml::to_string_pretty(&config).unwrap();
    assert!(
        !at_rest.contains("sentinel-"),
        "plaintext credential at rest:\n{at_rest}"
    );

    decrypt_config_secrets(&mut config, dir.path()).unwrap();
    assert_eq!(
        toml::to_string_pretty(&config).unwrap(),
        toml::to_string_pretty(&plain).unwrap()
    );
}

/// An empty `hosting.api_key` means "read the environment"; sealing it would
/// turn it into a non-empty value that is no key at all.
#[test]
fn an_empty_hosting_key_stays_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.config_path = dir.path().join("config.toml");
    config.secrets.encrypt = true;

    config.integrations.tinyfish.api_key = Some(String::new());

    encrypt_config_secrets(&mut config).unwrap();
    assert_eq!(config.hosting.api_key, "");
    assert_eq!(config.integrations.tinyfish.api_key.as_deref(), Some(""));
}

/// A snapshot reload, which Composio tools run on every call, opens sealed
/// secrets just as the first load does.
#[tokio::test]
async fn a_snapshot_reload_opens_sealed_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.config_path = dir.path().join("config.toml");
    config.secrets.encrypt = true;
    config.composio.api_key = Some("composio-sentinel".into());
    encrypt_config_secrets(&mut config).unwrap();
    std::fs::write(
        &config.config_path,
        toml::to_string_pretty(&config).unwrap(),
    )
    .unwrap();

    let reloaded =
        Config::load_from_config_path(&config.config_path, &dir.path().join("workspace"))
            .await
            .unwrap();
    assert_eq!(
        reloaded.composio.api_key.as_deref(),
        Some("composio-sentinel")
    );
}
