//! Checks that explain configuration overrides and their non-secret sources.

use async_trait::async_trait;

use crate::config::schema::load::ProvenanceSource;

use super::super::contract::{
    DoctorCheck, DoctorCtx, DoctorTier, Finding, FindingSeverity, ValueSource,
};

const CATEGORY: &str = "configuration";

pub(super) struct EnvironmentOverrideCheck;

#[async_trait]
impl DoctorCheck for EnvironmentOverrideCheck {
    fn id(&self) -> &str {
        "config/env-override"
    }

    fn category(&self) -> &str {
        CATEGORY
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Static
    }

    async fn detect(&self, ctx: &DoctorCtx) -> Vec<Finding> {
        ctx.config()
            .load_provenance()
            .fields()
            .filter_map(|(path, provenance)| {
                let source = match provenance.source {
                    ProvenanceSource::Env => ValueSource::Env,
                    ProvenanceSource::Dotenv => ValueSource::Dotenv,
                    ProvenanceSource::Cli | ProvenanceSource::File | ProvenanceSource::Default => {
                        return None;
                    }
                };
                let env_var = provenance.env_var.as_deref()?;
                let severity = if provenance.file_also_set {
                    FindingSeverity::Warn
                } else {
                    FindingSeverity::Info
                };
                let message = if provenance.file_also_set {
                    format!(
                        "Environment variable `{env_var}` overrides a value explicitly set in the config file."
                    )
                } else {
                    format!(
                        "Environment variable `{env_var}` supplies this value; the config file does not set it explicitly."
                    )
                };
                let mut finding = Finding::new(
                    self.id(),
                    severity,
                    message,
                    "Choose one source for this setting and remove the shadowed override.",
                );
                finding.config_path = Some(path.to_owned());
                finding.value_source = Some(source);
                Some(finding)
            })
            .collect()
    }
}

pub(super) struct ConfigSourceCheck;

#[async_trait]
impl DoctorCheck for ConfigSourceCheck {
    fn id(&self) -> &str {
        "config/source"
    }

    fn category(&self) -> &str {
        CATEGORY
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Static
    }

    async fn detect(&self, ctx: &DoctorCtx) -> Vec<Finding> {
        let config = ctx.config();
        let provenance = config.load_provenance();
        let mut message = format!("Config file: {}.", config.config_path.display());

        if let Some(dotenv) = provenance.dotenv.as_ref() {
            message.push_str(&format!(" .env file: {}.", dotenv.path.display()));
            if !dotenv.skipped_vars.is_empty() {
                let mut shadowed_vars = dotenv.skipped_vars.clone();
                shadowed_vars.sort();
                shadowed_vars.dedup();
                message.push_str(&format!(
                    " .env variables shadowed by the process environment: {}.",
                    shadowed_vars.join(", ")
                ));
            }
        } else {
            message.push_str(" No .env file was loaded.");
        }

        let cli_override_active = provenance
            .fields()
            .any(|(_, field)| field.source == ProvenanceSource::Cli);
        message.push_str(&format!(
            " CLI `--model`/`--provider` inference override active: {cli_override_active}."
        ));

        let mut finding = Finding::new(
            self.id(),
            FindingSeverity::Info,
            message,
            "No action required; use these sources when diagnosing configuration precedence.",
        );
        finding.config_path = Some(config.config_path.display().to_string());
        vec![finding]
    }
}
