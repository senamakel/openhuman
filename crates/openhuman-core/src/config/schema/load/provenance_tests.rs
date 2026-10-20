use super::{ConfigProvenance, DotenvLoadProvenance, ProvenanceSource};
use std::path::PathBuf;

#[test]
fn environment_override_reports_source_and_whether_the_file_also_set_it() {
    let document: toml::Value = toml::from_str(r#"default_model = "file-model""#).unwrap();
    let mut provenance = ConfigProvenance::default();
    provenance.record_file_keys(&document);
    provenance.record_env_override(
        "default_model",
        "OPENHUMAN_DEFAULT_MODEL",
        ProvenanceSource::Env,
    );

    let entry = provenance.field("default_model");
    assert_eq!(entry.source, ProvenanceSource::Env);
    assert_eq!(entry.env_var.as_deref(), Some("OPENHUMAN_DEFAULT_MODEL"));
    assert!(entry.file_also_set);
    assert!(!format!("{provenance:?}").contains("file-model"));
}

#[test]
fn unoverridden_values_report_file_or_default_without_values() {
    let document: toml::Value = toml::from_str(
        r#"
default_model = "file-model"
[agent]
tool_dispatcher = "native"
"#,
    )
    .unwrap();
    let provenance = ConfigProvenance::from_file_document(&document);

    assert_eq!(provenance.field("default_model").source, ProvenanceSource::File);
    assert_eq!(
        provenance.field("agent.tool_dispatcher").source,
        ProvenanceSource::File
    );
    assert_eq!(provenance.field("output_language").source, ProvenanceSource::Default);
    assert!(!format!("{provenance:?}").contains("file-model"));
    assert!(!format!("{provenance:?}").contains("native"));
}

#[test]
fn dotenv_record_keeps_the_loaded_path_and_only_names_of_shadowed_variables() {
    let record = DotenvLoadProvenance::from_variables(
        PathBuf::from("/workspace/.env"),
        ["DOTENV_ONLY", "PROCESS_WINS"],
        ["PROCESS_WINS"],
    );

    assert_eq!(record.path, PathBuf::from("/workspace/.env"));
    assert_eq!(record.skipped_vars, ["PROCESS_WINS"]);
    assert_eq!(record.loaded_vars, ["DOTENV_ONLY"]);
    assert!(!format!("{record:?}").contains("secret"));
}
