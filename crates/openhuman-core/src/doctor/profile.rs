//! Read-only loading and existence diagnostics for desktop profiles.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use crate::config::schema::profile_layout::LayoutVersion;
use crate::config::Config;
use crate::core::runtime::ComponentSet;
use crate::doctor::contract::{DoctorCheck, DoctorCtx, DoctorProfileStatus, DoctorTier, Finding};
use crate::profiles::ProfileLayout;

const EXPLICIT_PROFILE_SOURCE: &str = "--profile";

/// A desktop profile config loaded without activating or repairing it.
#[derive(Debug, Clone)]
pub(crate) struct DesktopProfile {
    pub id: String,
    pub layout: ProfileLayout,
    pub config: Config,
}

/// Safe failure categories for a selected desktop profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesktopProfileError {
    InvalidId,
    Missing,
    UnknownLayoutMarker,
    InvalidConfig,
}

impl DesktopProfileError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::InvalidId => "The selected profile id is invalid.",
            Self::Missing => "The selected profile does not exist.",
            Self::UnknownLayoutMarker => "The selected profile has an unsupported layout marker.",
            Self::InvalidConfig => "The selected profile config is missing or invalid.",
        }
    }
}

/// Loads one profile beneath `root` without changing active-user state,
/// creating directories, writing a layout marker, applying migrations, or
/// recovering from a backup. Process environment config overrides still
/// apply after the selected profile file is parsed.
pub(crate) async fn load_desktop_profile(
    root: &Path,
    id: &str,
) -> Result<DesktopProfile, DesktopProfileError> {
    if !is_profile_path_segment(id) {
        return Err(DesktopProfileError::InvalidId);
    }

    let dir = ProfileLayout::new(root, id).dir;
    let metadata = tokio::fs::metadata(&dir).await.map_err(|error| {
        if matches!(
            error.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
        ) {
            DesktopProfileError::Missing
        } else {
            DesktopProfileError::InvalidConfig
        }
    })?;
    if !metadata.is_dir() {
        return Err(DesktopProfileError::Missing);
    }

    let version = read_layout_version(&dir).await?;
    let layout = ProfileLayout::with_version(dir, version);
    let config_text = tokio::fs::read_to_string(&layout.config_path)
        .await
        .map_err(|_| DesktopProfileError::InvalidConfig)?;
    let mut config = crate::config::config_from_toml_str(&config_text)
        .map_err(|_| DesktopProfileError::InvalidConfig)?;
    config.config_path = layout.config_path.clone();
    config.workspace_dir = layout.workspace_dir.clone();
    config.action_dir = crate::config::resolve_action_dir(&config.action_dir_override);
    config.apply_env_overrides();

    Ok(DesktopProfile {
        id: id.to_owned(),
        layout,
        config,
    })
}

/// Derives a selected profile's root from an already-resolved config path.
/// This does not consult the active-profile markers or environment.
pub(crate) fn root_for_config(config: &Config) -> Option<PathBuf> {
    let config_dir = config.config_path.parent()?;
    let users_dir = config_dir.parent()?;
    if users_dir.file_name().is_some_and(|name| name == "users") {
        return users_dir.parent().map(Path::to_path_buf);
    }
    Some(config_dir.to_path_buf())
}

/// Profile id represented by a config path, if it is in `<root>/users/<id>`.
pub(crate) fn id_for_config(config: &Config) -> Option<String> {
    let config_dir = config.config_path.parent()?;
    let users_dir = config_dir.parent()?;
    if !users_dir.file_name().is_some_and(|name| name == "users") {
        return None;
    }
    let id = config_dir.file_name()?.to_str()?;
    is_profile_path_segment(id).then(|| id.to_owned())
}

pub(crate) fn is_profile_path_segment(id: &str) -> bool {
    if id.is_empty() || id.chars().any(char::is_control) || id.contains(['/', '\\']) {
        return false;
    }
    let mut components = Path::new(id).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

async fn read_layout_version(dir: &Path) -> Result<LayoutVersion, DesktopProfileError> {
    let marker = dir.join(".layout");
    match tokio::fs::read_to_string(&marker).await {
        Ok(value) if value.trim() == "2" => Ok(LayoutVersion::V2),
        Ok(_) => Err(DesktopProfileError::UnknownLayoutMarker),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(LayoutVersion::detect(dir))
        }
        Err(_) => Err(DesktopProfileError::UnknownLayoutMarker),
    }
}

pub(crate) fn built_in_checks() -> Vec<Arc<dyn DoctorCheck>> {
    vec![Arc::new(ProfileExistsCheck)]
}

struct ProfileExistsCheck;

#[async_trait]
impl DoctorCheck for ProfileExistsCheck {
    fn id(&self) -> &str {
        "profile/exists"
    }

    fn category(&self) -> &str {
        "profile"
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Static
    }

    async fn detect(&self, ctx: &DoctorCtx) -> Vec<Finding> {
        if ctx.config_source() == super::contract::DoctorConfigSource::Tenant {
            return vec![Finding::skipped(
                self.id(),
                "Tenant profile readiness is reported by the hosted profile checks.",
                "No action required.",
            )];
        }
        match ctx.profile_status() {
            Some(DoctorProfileStatus::Valid) => {
                vec![Finding::pass(
                    self.id(),
                    "The selected profile exists and its config is valid.",
                )]
            }
            Some(DoctorProfileStatus::Missing) => vec![Finding::error(
                self.id(),
                DesktopProfileError::Missing.message(),
                "Select an existing desktop profile and rerun doctor.",
            )],
            Some(DoctorProfileStatus::UnknownLayoutMarker) => vec![Finding::error(
                self.id(),
                DesktopProfileError::UnknownLayoutMarker.message(),
                "Use a profile with a supported layout marker and rerun doctor.",
            )],
            Some(DoctorProfileStatus::InvalidConfig) => vec![Finding::error(
                self.id(),
                DesktopProfileError::InvalidConfig.message(),
                "Repair the selected profile config and rerun doctor.",
            )],
            None => vec![Finding::skipped(
                self.id(),
                "No desktop profile was selected for this run.",
                "Pass --profile <id> to validate a specific profile.",
            )],
        }
    }
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
