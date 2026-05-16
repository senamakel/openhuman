use super::client::{
    McpAuthorizationContext, McpHttpClient, McpInitializeResult, McpRemoteTool, McpServerToolResult,
};
use crate::openhuman::config::{Config, McpAuthConfig, McpClientIdentityConfig, McpServerConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpRegistrySource {
    Config,
    LegacyGitbooks,
}

#[derive(Debug, Clone)]
pub struct McpServerDefinition {
    pub name: String,
    pub endpoint: String,
    pub description: Option<String>,
    pub timeout_secs: u64,
    pub auth: McpAuthConfig,
    pub source: McpRegistrySource,
    client: Arc<McpHttpClient>,
}

#[derive(Debug, Default, Clone)]
pub struct McpServerRegistry {
    by_name: HashMap<String, McpServerDefinition>,
    order: Vec<String>,
}

impl McpServerRegistry {
    pub fn from_config(config: &Config) -> Self {
        let mut registry = Self::default();
        if !config.mcp_client.enabled {
            return registry;
        }

        for server in &config.mcp_client.servers {
            registry.register_config_server(
                server,
                &config.mcp_client.client_identity,
                McpRegistrySource::Config,
            );
        }

        if config.gitbooks.enabled && registry.get("gitbooks").is_none() {
            registry.insert(McpServerDefinition {
                name: "gitbooks".into(),
                endpoint: config.gitbooks.endpoint.clone(),
                description: Some("OpenHuman GitBook documentation MCP server.".into()),
                timeout_secs: config.gitbooks.timeout_secs,
                auth: McpAuthConfig::None,
                source: McpRegistrySource::LegacyGitbooks,
                client: Arc::new(McpHttpClient::with_options(
                    config.gitbooks.endpoint.clone(),
                    config.gitbooks.timeout_secs,
                    McpAuthConfig::None,
                    config.mcp_client.client_identity.clone(),
                )),
            });
        }

        registry
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn list(&self) -> Vec<&McpServerDefinition> {
        self.order
            .iter()
            .filter_map(|name| self.by_name.get(name))
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<&McpServerDefinition> {
        self.by_name.get(name)
    }

    pub async fn list_tools(&self, server: &str) -> anyhow::Result<Vec<McpRemoteTool>> {
        let server = self
            .get(server)
            .ok_or_else(|| anyhow::anyhow!("unknown MCP server `{server}`"))?;
        server.client.list_tools().await
    }

    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: Value,
    ) -> anyhow::Result<McpServerToolResult> {
        let server = self
            .get(server)
            .ok_or_else(|| anyhow::anyhow!("unknown MCP server `{server}`"))?;
        server.client.call_tool(tool, arguments).await
    }

    pub async fn initialize(&self, server: &str) -> anyhow::Result<McpInitializeResult> {
        let server = self
            .get(server)
            .ok_or_else(|| anyhow::anyhow!("unknown MCP server `{server}`"))?;
        server.client.initialize().await
    }

    pub async fn discover_authorization(
        &self,
        server: &str,
    ) -> anyhow::Result<Option<McpAuthorizationContext>> {
        let server = self
            .get(server)
            .ok_or_else(|| anyhow::anyhow!("unknown MCP server `{server}`"))?;
        server.client.discover_authorization().await
    }

    fn register_config_server(
        &mut self,
        server: &McpServerConfig,
        identity: &McpClientIdentityConfig,
        source: McpRegistrySource,
    ) {
        if !server.enabled {
            return;
        }
        let name = server.name.trim();
        let endpoint = server.endpoint.trim();
        if name.is_empty() || endpoint.is_empty() {
            tracing::warn!(
                name = server.name,
                endpoint = server.endpoint,
                "[mcp_client] skipping malformed MCP server config entry"
            );
            return;
        }
        self.insert(McpServerDefinition {
            name: name.to_string(),
            endpoint: endpoint.to_string(),
            description: server.description.clone(),
            timeout_secs: server.timeout_secs,
            auth: server.auth.clone(),
            source,
            client: Arc::new(McpHttpClient::with_options(
                endpoint.to_string(),
                server.timeout_secs,
                server.auth.clone(),
                identity.clone(),
            )),
        });
    }

    fn insert(&mut self, def: McpServerDefinition) {
        let name = def.name.clone();
        if self.by_name.insert(name.clone(), def).is_none() {
            self.order.push(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_seeds_legacy_gitbooks_when_enabled() {
        let config = Config::default();
        let registry = McpServerRegistry::from_config(&config);
        let gitbooks = registry.get("gitbooks").expect("gitbooks");
        assert_eq!(gitbooks.source, McpRegistrySource::LegacyGitbooks);
    }

    #[test]
    fn explicit_server_overrides_legacy_name() {
        let mut config = Config::default();
        config.mcp_client.servers.push(McpServerConfig {
            name: "gitbooks".into(),
            endpoint: "https://example.com/mcp".into(),
            description: Some("Custom docs".into()),
            enabled: true,
            timeout_secs: 9,
            auth: crate::openhuman::config::McpAuthConfig::None,
        });
        let registry = McpServerRegistry::from_config(&config);
        let gitbooks = registry.get("gitbooks").expect("gitbooks");
        assert_eq!(gitbooks.source, McpRegistrySource::Config);
        assert_eq!(gitbooks.endpoint, "https://example.com/mcp");
    }

    #[test]
    fn disabled_config_short_circuits_registry() {
        let mut config = Config::default();
        config.mcp_client.enabled = false;
        let registry = McpServerRegistry::from_config(&config);
        assert!(registry.is_empty());
    }
}
