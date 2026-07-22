use serde::{Deserialize, Serialize};

use crate::openhuman::agent_experience::store::{AgentExperienceStore, ExperienceQuery};
use crate::openhuman::agent_experience::types::{AgentExperience, ExperienceHit};
use crate::openhuman::config::Config;
use crate::rpc::RpcOutcome;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct CaptureParams {
    pub experience: AgentExperience,
}

#[derive(Debug, Deserialize, Default)]
pub struct RetrieveParams {
    pub query: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub entrypoint: Option<String>,
    /// Profile partition filter (1c). `None` (omitted) recalls the whole pool;
    /// `Some(P)` recalls records stamped `P` plus unstamped legacy records.
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub max_hits: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ListParams {
    /// Profile partition filter (1c), same semantics as `RetrieveParams`.
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DismissParams {
    pub id: String,
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DismissResult {
    pub id: String,
    pub dismissed: bool,
}

fn profile_memory_subdir(
    workspace_dir: &std::path::Path,
    profile_id: Option<&str>,
) -> Result<String, String> {
    let Some(profile_id) = profile_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok("memory".to_string());
    };
    let state = crate::openhuman::profiles::load_profiles(workspace_dir)?;
    let profile = state
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| format!("agent profile '{profile_id}' not found"))?;
    let suffix = crate::openhuman::profiles::effective_memory_suffix(profile);
    Ok(crate::openhuman::profiles::memory_subdir_for_suffix(
        &suffix,
    ))
}

async fn open_store(profile_id: Option<&str>) -> Result<AgentExperienceStore, String> {
    let profile_id = profile_id.map(str::trim).filter(|id| !id.is_empty());
    if profile_id.is_none() {
        let client = match crate::openhuman::memory::global::client_if_ready() {
            Some(client) => client,
            None => {
                let config = Config::load_or_init()
                    .await
                    .map_err(|e| format!("load config: {e}"))?;
                crate::openhuman::memory::global::init(config.workspace_dir)?
            }
        };
        return Ok(AgentExperienceStore::new(client.memory_handle()));
    }

    let config = Config::load_or_init()
        .await
        .map_err(|e| format!("load config: {e}"))?;
    let memory_subdir = profile_memory_subdir(&config.workspace_dir, profile_id)?;

    // Keep the shared-memory path on the process-global client. Profile memory
    // uses the same concrete store layout as the session builder, but opens the
    // profile-derived subtree so RPC capture/retrieve/list/dismiss see the
    // records learned by that profile's live agent sessions.
    if memory_subdir != "memory" {
        let memory = crate::openhuman::memory_store::UnifiedMemory::new_with_memory_dir(
            &config.workspace_dir,
            &memory_subdir,
            crate::openhuman::embeddings::default_embedding_provider(),
            config.memory.sqlite_open_timeout_secs,
        )
        .map_err(|e| format!("open agent experience store '{memory_subdir}': {e:#}"))?;
        return Ok(AgentExperienceStore::new(Arc::new(memory)));
    }

    let client = match crate::openhuman::memory::global::client_if_ready() {
        Some(client) => client,
        None => crate::openhuman::memory::global::init(config.workspace_dir)?,
    };
    Ok(AgentExperienceStore::new(client.memory_handle()))
}

pub async fn capture(params: CaptureParams) -> Result<RpcOutcome<AgentExperience>, String> {
    let store = open_store(params.experience.profile_id.as_deref()).await?;
    let stored = store.put(params.experience).await?;
    Ok(RpcOutcome::single_log(stored, "agent experience captured"))
}

pub async fn retrieve(params: RetrieveParams) -> Result<RpcOutcome<Vec<ExperienceHit>>, String> {
    let store = open_store(params.profile_id.as_deref()).await?;
    let hits = store
        .retrieve(ExperienceQuery {
            query: params.query,
            tools: params.tools,
            tags: params.tags,
            agent_id: params.agent_id,
            entrypoint: params.entrypoint,
            profile_id: params.profile_id,
            max_hits: params.max_hits.unwrap_or(5),
        })
        .await?;
    Ok(RpcOutcome::single_log(hits, "agent experiences retrieved"))
}

pub async fn list(params: ListParams) -> Result<RpcOutcome<Vec<AgentExperience>>, String> {
    let store = open_store(params.profile_id.as_deref()).await?;
    let experiences = store.list_for_profile(params.profile_id.as_deref()).await?;
    Ok(RpcOutcome::single_log(
        experiences,
        "agent experiences listed",
    ))
}

pub async fn dismiss(params: DismissParams) -> Result<RpcOutcome<DismissResult>, String> {
    let store = open_store(params.profile_id.as_deref()).await?;
    let dismissed = store.dismiss(&params.id).await?;
    Ok(RpcOutcome::single_log(
        DismissResult {
            id: params.id,
            dismissed,
        },
        "agent experience dismissed",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_memory_subdir_matches_live_session_derivation() {
        let workspace = tempfile::TempDir::new().unwrap();
        let mut profile = crate::openhuman::profiles::store::built_in_default_profile();
        profile.id = "alice".into();
        profile.name = "Alice".into();
        profile.built_in = false;
        profile.is_master = false;
        profile.dedicated_memory = true;
        crate::openhuman::profiles::store::AgentProfileStore::new(workspace.path().to_path_buf())
            .upsert(profile)
            .expect("seed profile");

        assert_eq!(
            profile_memory_subdir(workspace.path(), Some("alice")).unwrap(),
            "memory-alice"
        );
        assert_eq!(
            profile_memory_subdir(workspace.path(), None).unwrap(),
            "memory"
        );
        assert!(profile_memory_subdir(workspace.path(), Some("missing")).is_err());
    }
}
