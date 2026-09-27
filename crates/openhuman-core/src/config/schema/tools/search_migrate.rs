//! One-time migration from the single-engine `[search]` format to providers,
//! routes and roles.

use std::collections::BTreeMap;

use super::{
    LegacySearchInputs, SearchConfig, SearchProviderSettings, SearchRoute, SEARCH_ENGINE_BRAVE,
    SEARCH_ENGINE_DISABLED, SEARCH_ENGINE_EXA, SEARCH_ENGINE_MANAGED, SEARCH_ENGINE_PARALLEL,
    SEARCH_ENGINE_QUERIT, SEARCH_ENGINE_TAVILY, SEARCH_PROVIDERS, SEARCH_ROLE_SEARCH,
    SEARCH_SCHEMA_VERSION,
};

impl SearchConfig {
    /// Whether this file still carries the single-engine format.
    pub fn needs_migration(&self) -> bool {
        self.schema_version < SEARCH_SCHEMA_VERSION
    }

    /// Convert the single-engine format into providers, routes and roles.
    /// Idempotent: returns `false` and changes nothing on a current file.
    ///
    /// Managed selections map to managed Exa (search, contents) plus managed
    /// Gemini (answer) regardless of whether a session exists right now; the
    /// settings RPC reports them as "sign in required" until one does.
    /// Parallel stays as a bring-your-own-key provider. Only managed
    /// (backend-routed) Parallel is gone: a selection that relied on it
    /// without a key is dropped, and Exa and Gemini cover those roles.
    pub fn migrate_legacy(&mut self, legacy: LegacySearchInputs) -> bool {
        if !self.needs_migration() {
            return false;
        }
        let engine = self
            .engine
            .as_deref()
            .map(|e| e.trim().to_ascii_lowercase())
            .unwrap_or_else(|| SEARCH_ENGINE_MANAGED.to_string());
        let enabled = self.enabled.unwrap_or(engine != SEARCH_ENGINE_DISABLED);
        let gemini_route = self
            .gemini_route
            .as_deref()
            .and_then(SearchRoute::parse)
            .unwrap_or(SearchRoute::Managed);
        let mut providers = BTreeMap::new();
        let parallel_key = self.parallel.has_key();
        let managed_parallel = self.parallel_route.as_deref().and_then(SearchRoute::parse)
            == Some(SearchRoute::Managed);
        let mut dropped_parallel = false;

        match self.enabled_providers.take() {
            Some(selected) => {
                let selected_exa_with_key = selected.contains("exa") && self.exa.has_key();
                for name in selected {
                    match name.as_str() {
                        "managed" => {
                            providers.insert(
                                "exa".into(),
                                if selected_exa_with_key {
                                    SearchProviderSettings::direct()
                                } else {
                                    SearchProviderSettings::managed()
                                },
                            );
                            providers
                                .entry("gemini".into())
                                .or_insert_with(SearchProviderSettings::managed);
                        }
                        "parallel" if parallel_key || !managed_parallel => {
                            providers.insert("parallel".into(), SearchProviderSettings::direct());
                        }
                        "parallel" => dropped_parallel = true,
                        "gemini" => {
                            providers.insert(
                                "gemini".into(),
                                SearchProviderSettings {
                                    enabled: true,
                                    route: gemini_route,
                                },
                            );
                        }
                        "tinyfish" => {
                            providers.insert("tinyfish".into(), SearchProviderSettings::managed());
                        }
                        other if SEARCH_PROVIDERS.contains(&other) => {
                            providers
                                .entry(other.to_string())
                                .or_insert_with(SearchProviderSettings::direct);
                        }
                        other => {
                            tracing::warn!(
                                provider = other,
                                "[config][migrate][search] dropping unknown provider"
                            );
                        }
                    }
                }
            }
            None => {
                let exa_route = if engine == SEARCH_ENGINE_EXA && self.exa.has_key() {
                    SearchRoute::Direct
                } else {
                    SearchRoute::Managed
                };
                providers.insert(
                    "exa".into(),
                    SearchProviderSettings {
                        enabled: true,
                        route: exa_route,
                    },
                );
                let gemini_route = if self.gemini.has_key() && self.gemini_route.is_some() {
                    gemini_route
                } else {
                    SearchRoute::Managed
                };
                providers.insert(
                    "gemini".into(),
                    SearchProviderSettings {
                        enabled: true,
                        route: gemini_route,
                    },
                );
                for (name, credentials) in [
                    ("brave", &self.brave),
                    ("querit", &self.querit),
                    ("tavily", &self.tavily),
                    ("parallel", &self.parallel),
                ] {
                    if credentials.has_key() {
                        providers.insert(name.into(), SearchProviderSettings::direct());
                    }
                }
                if legacy.tinyfish_active {
                    providers.insert("tinyfish".into(), SearchProviderSettings::managed());
                }
                if legacy.seltz_active {
                    providers.insert("seltz".into(), SearchProviderSettings::direct());
                }
                if legacy.searxng_active {
                    providers.insert("searxng".into(), SearchProviderSettings::direct());
                }
            }
        }

        // These legacy toggles were independent of enabled_providers.
        if legacy.searxng_active {
            providers.insert("searxng".into(), SearchProviderSettings::direct());
        }
        if legacy.seltz_active {
            providers.insert("seltz".into(), SearchProviderSettings::direct());
        }
        if legacy.tinyfish_active {
            providers.insert("tinyfish".into(), SearchProviderSettings::managed());
        }

        // A deliberately chosen BYO engine stays first for ranked search.
        let mut roles = BTreeMap::new();
        if matches!(
            engine.as_str(),
            SEARCH_ENGINE_BRAVE
                | SEARCH_ENGINE_QUERIT
                | SEARCH_ENGINE_TAVILY
                | SEARCH_ENGINE_PARALLEL
        ) && providers.contains_key(engine.as_str())
        {
            roles.insert(
                SEARCH_ROLE_SEARCH.to_string(),
                vec![engine.clone(), "exa".into()],
            );
        }

        if dropped_parallel {
            tracing::warn!(
                "[config][migrate][search] managed Parallel is no longer offered and no \
                 Parallel key is stored; dropped it (managed Exa and Gemini cover its roles, \
                 or add your own Parallel key)"
            );
        }
        tracing::info!(
            engine = %engine,
            enabled,
            providers = ?providers.keys().collect::<Vec<_>>(),
            "[config][migrate][search] migrated single-engine search settings"
        );

        self.enabled = Some(enabled);
        self.providers = providers;
        self.roles = roles;
        if self.presentation_provider.as_deref() == Some("managed")
            || (self.presentation_provider.as_deref() == Some("parallel")
                && !self.providers.contains_key("parallel"))
        {
            self.presentation_provider = None;
        }
        // `roles` did not exist in the legacy presentation vocabulary; its
        // serde default means the old file omitted the field. Preserve the
        // legacy default while fresh configs use Roles.
        if self.presentation == super::SearchPresentation::Roles {
            self.presentation = super::SearchPresentation::AllTools;
        }
        self.engine = None;
        self.parallel_route = None;
        self.gemini_route = None;
        self.schema_version = SEARCH_SCHEMA_VERSION;
        true
    }

    /// Apply a legacy single-engine selection (`SEARCH_ENGINE`, or an older
    /// client sending `engine`) on top of the current provider settings.
    pub fn apply_legacy_engine(&mut self, engine: &str) -> Result<(), String> {
        let engine = engine.trim().to_ascii_lowercase();
        match engine.as_str() {
            SEARCH_ENGINE_DISABLED => {
                self.enabled = Some(false);
            }
            SEARCH_ENGINE_MANAGED => {
                self.enabled = Some(true);
                self.providers
                    .insert("exa".into(), SearchProviderSettings::managed());
                self.providers
                    .entry("gemini".into())
                    .or_insert_with(SearchProviderSettings::managed);
                self.roles.remove(SEARCH_ROLE_SEARCH);
            }
            SEARCH_ENGINE_BRAVE
            | SEARCH_ENGINE_QUERIT
            | SEARCH_ENGINE_TAVILY
            | SEARCH_ENGINE_PARALLEL
            | SEARCH_ENGINE_EXA => {
                self.enabled = Some(true);
                self.providers
                    .insert(engine.clone(), SearchProviderSettings::direct());
                let mut order = vec![engine.clone()];
                if engine != SEARCH_ENGINE_EXA {
                    order.push("exa".into());
                }
                self.roles.insert(SEARCH_ROLE_SEARCH.to_string(), order);
            }
            other => {
                return Err(format!(
                    "unknown search engine '{other}' (expected disabled, managed, brave, querit, exa, tavily or parallel)"
                ));
            }
        }
        tracing::debug!(engine = %engine, "[config][search] applied legacy engine selection");
        Ok(())
    }
}
