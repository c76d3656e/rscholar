//! Server configuration from `config.toml`.
//!
//! Centralized configuration for all services.

use crate::error::{GscholarError, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use tracing::{info, warn};

/// Root configuration structure
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub server: ServerSection,
    pub easyscholar: EasyScholarSection,
    #[serde(default)]
    pub ranking: RankingSection,
    pub llm: LlmSection,
    pub search: SearchSection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerSection {
    pub host: String,
    pub port: u16,
    #[serde(default = "default_admin_enabled")]
    pub admin_enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EasyScholarSection {
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RankingSection {
    #[serde(default = "default_ranking_queue_capacity")]
    pub queue_capacity: usize,
    #[serde(default = "default_ranking_min_chunk")]
    pub min_chunk: usize,
    #[serde(default = "default_ranking_max_chunk")]
    pub max_chunk: usize,
    #[serde(default = "default_ranking_max_concurrent_jobs")]
    pub max_concurrent_jobs: usize,
    #[serde(default = "default_ranking_scheduler_mode")]
    pub scheduler_mode: String,
    #[serde(default = "default_ranking_target_duration_sec")]
    pub target_duration_sec: u64,
    #[serde(default = "default_ranking_eta_scale")]
    pub eta_scale: f64,
    #[serde(default = "default_ranking_heartbeat_ms")]
    pub heartbeat_ms: u64,
    #[serde(default = "default_ranking_job_timeout_min_sec")]
    pub job_timeout_min_sec: u64,
    #[serde(default = "default_ranking_key_fail_threshold")]
    pub key_fail_threshold: usize,
    #[serde(default = "default_ranking_key_cooldown_sec")]
    pub key_cooldown_sec: u64,
    #[serde(default = "default_ranking_key_stale_ttl_sec")]
    pub key_stale_ttl_sec: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmSection {
    pub default_provider: String,
    pub enable_filter: bool,
    #[serde(default)]
    pub strict_filter: bool,
    #[serde(default)]
    pub providers: Vec<String>,
    #[serde(default, rename = "registry")]
    pub provider_configs: HashMap<String, ProviderConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchSection {
    pub default_ylo: Option<i32>,
    pub enable_crossref: bool,
    /// Deprecated: prefer per-source `enabled` flags in [search.xxx] sections.
    /// If set and non-empty, overrides the per-source flags for pipeline.
    #[serde(default)]
    pub enabled_sources: Vec<String>,
    /// Kept for backward compat; prefer openalex.max_results
    #[serde(default = "default_ss_limit")]
    pub ss_limit: usize,
    /// Kept for backward compat; prefer semanticscholar.max_results
    #[serde(default = "default_oa_limit")]
    pub oa_limit: usize,
    #[serde(default)]
    pub openalex: SearchOpenAlexSection,
    #[serde(default)]
    pub semanticscholar: SearchSemanticScholarSection,
    #[serde(default)]
    pub arxiv: SearchArxivSection,
    #[serde(default)]
    pub pubmed: SearchPubMedSection,
    #[serde(default)]
    pub xrxiv: SearchXRxivSection,
}

impl SearchSection {
    /// Derive the effective enabled-source list from per-section `enabled` flags.
    /// Falls back to explicit `enabled_sources` if set (backward compat).
    pub fn effective_sources(&self) -> Vec<String> {
        if !self.enabled_sources.is_empty() {
            return self.enabled_sources.clone();
        }
        let mut out = Vec::new();
        if self.openalex.enabled { out.push("openalex".to_string()); }
        if self.semanticscholar.enabled { out.push("semanticscholar".to_string()); }
        if self.arxiv.enabled { out.push("arxiv".to_string()); }
        if self.pubmed.enabled { out.push("pubmed".to_string()); }
        if self.xrxiv.enabled {
            out.push("biorxiv".to_string());
            out.push("medrxiv".to_string());
        }
        out
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchOpenAlexSection {
    #[serde(default = "default_source_enabled")]
    pub enabled: bool,
    #[serde(default = "default_oa_limit")]
    pub max_results: usize,
    #[serde(default = "default_source_timeout_sec")]
    pub timeout_sec: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchSemanticScholarSection {
    #[serde(default = "default_source_enabled")]
    pub enabled: bool,
    #[serde(default = "default_ss_limit")]
    pub max_results: usize,
    #[serde(default = "default_source_timeout_sec")]
    pub timeout_sec: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchArxivSection {
    #[serde(default = "default_source_enabled")]
    pub enabled: bool,
    #[serde(default = "default_source_max_results")]
    pub max_results: usize,
    #[serde(default = "default_arxiv_page_size")]
    pub page_size: usize,
    #[serde(default = "default_arxiv_sort_by")]
    pub sort_by: String,
    #[serde(default = "default_arxiv_sort_order")]
    pub sort_order: String,
    #[serde(default = "default_source_timeout_sec")]
    pub timeout_sec: u64,
    #[serde(default = "default_arxiv_delay_ms")]
    pub request_delay_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchPubMedSection {
    #[serde(default = "default_source_enabled")]
    pub enabled: bool,
    #[serde(default = "default_source_max_results")]
    pub max_results: usize,
    #[serde(default = "default_pubmed_page_size")]
    pub page_size: usize,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_pubmed_tool")]
    pub tool: String,
    #[serde(default = "default_pubmed_email")]
    pub email: String,
    #[serde(default = "default_source_timeout_sec")]
    pub timeout_sec: u64,
    #[serde(default = "default_pubmed_delay_no_key_ms")]
    pub delay_no_key_ms: u64,
    #[serde(default = "default_pubmed_delay_with_key_ms")]
    pub delay_with_key_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchXRxivSection {
    #[serde(default = "default_source_enabled")]
    pub enabled: bool,
    #[serde(default = "default_source_max_results")]
    pub biorxiv_max_results: usize,
    #[serde(default = "default_source_max_results")]
    pub medrxiv_max_results: usize,
    #[serde(default = "default_xrxiv_start_date")]
    pub start_date: String,
    #[serde(default = "default_xrxiv_end_date")]
    pub end_date: String,
    #[serde(default = "default_source_timeout_sec")]
    pub timeout_sec: u64,
    #[serde(default = "default_xrxiv_delay_ms")]
    pub request_delay_ms: u64,
    #[serde(default = "default_xrxiv_max_retries")]
    pub max_retries: usize,
}

fn default_ranking_queue_capacity() -> usize {
    128
}

fn default_admin_enabled() -> bool {
    false
}

fn default_ranking_min_chunk() -> usize {
    1
}

fn default_ranking_max_chunk() -> usize {
    32
}

fn default_ranking_max_concurrent_jobs() -> usize {
    16
}

fn default_ranking_scheduler_mode() -> String {
    "easy_backfill".to_string()
}

fn default_ranking_target_duration_sec() -> u64 {
    10
}

fn default_ranking_eta_scale() -> f64 {
    1.6
}

fn default_ranking_heartbeat_ms() -> u64 {
    200
}

fn default_ranking_job_timeout_min_sec() -> u64 {
    15
}

fn default_ranking_key_fail_threshold() -> usize {
    3
}

fn default_ranking_key_cooldown_sec() -> u64 {
    30
}

fn default_ranking_key_stale_ttl_sec() -> u64 {
    180
}

fn default_enabled_sources() -> Vec<String> {
    vec!["openalex".to_string(), "semanticscholar".to_string()]
}

/// When a [search.xxx] section IS present in TOML, `enabled` defaults to true.
fn default_source_enabled() -> bool {
    true
}

fn default_ss_limit() -> usize {
    100
}

fn default_oa_limit() -> usize {
    200
}

fn default_source_max_results() -> usize {
    200
}

fn default_source_timeout_sec() -> u64 {
    30
}

fn default_arxiv_page_size() -> usize {
    100
}

fn default_arxiv_sort_by() -> String {
    "relevance".to_string()
}

fn default_arxiv_sort_order() -> String {
    "descending".to_string()
}

fn default_arxiv_delay_ms() -> u64 {
    3000
}

fn default_pubmed_page_size() -> usize {
    100
}

fn default_pubmed_tool() -> String {
    "Rscholar".to_string()
}

fn default_pubmed_email() -> String {
    "c76d@c.com".to_string()
}

fn default_pubmed_delay_no_key_ms() -> u64 {
    350
}

fn default_pubmed_delay_with_key_ms() -> u64 {
    120
}

fn default_xrxiv_start_date() -> String {
    "2020-01-01".to_string()
}

fn default_xrxiv_end_date() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

fn default_xrxiv_delay_ms() -> u64 {
    500
}

fn default_xrxiv_max_retries() -> usize {
    3
}

impl Default for SearchOpenAlexSection {
    fn default() -> Self {
        Self {
            enabled: false,
            max_results: default_oa_limit(),
            timeout_sec: default_source_timeout_sec(),
        }
    }
}

impl Default for SearchSemanticScholarSection {
    fn default() -> Self {
        Self {
            enabled: false,
            max_results: default_ss_limit(),
            timeout_sec: default_source_timeout_sec(),
        }
    }
}

impl Default for SearchArxivSection {
    fn default() -> Self {
        Self {
            enabled: false,
            max_results: default_source_max_results(),
            page_size: default_arxiv_page_size(),
            sort_by: default_arxiv_sort_by(),
            sort_order: default_arxiv_sort_order(),
            timeout_sec: default_source_timeout_sec(),
            request_delay_ms: default_arxiv_delay_ms(),
        }
    }
}

impl Default for SearchPubMedSection {
    fn default() -> Self {
        Self {
            enabled: false,
            max_results: default_source_max_results(),
            page_size: default_pubmed_page_size(),
            api_key: String::new(),
            tool: default_pubmed_tool(),
            email: default_pubmed_email(),
            timeout_sec: default_source_timeout_sec(),
            delay_no_key_ms: default_pubmed_delay_no_key_ms(),
            delay_with_key_ms: default_pubmed_delay_with_key_ms(),
        }
    }
}

impl Default for SearchXRxivSection {
    fn default() -> Self {
        Self {
            enabled: false,
            biorxiv_max_results: default_source_max_results(),
            medrxiv_max_results: default_source_max_results(),
            start_date: default_xrxiv_start_date(),
            end_date: default_xrxiv_end_date(),
            timeout_sec: default_source_timeout_sec(),
            request_delay_ms: default_xrxiv_delay_ms(),
            max_retries: default_xrxiv_max_retries(),
        }
    }
}

impl Default for RankingSection {
    fn default() -> Self {
        Self {
            queue_capacity: default_ranking_queue_capacity(),
            min_chunk: default_ranking_min_chunk(),
            max_chunk: default_ranking_max_chunk(),
            max_concurrent_jobs: default_ranking_max_concurrent_jobs(),
            scheduler_mode: default_ranking_scheduler_mode(),
            target_duration_sec: default_ranking_target_duration_sec(),
            eta_scale: default_ranking_eta_scale(),
            heartbeat_ms: default_ranking_heartbeat_ms(),
            job_timeout_min_sec: default_ranking_job_timeout_min_sec(),
            key_fail_threshold: default_ranking_key_fail_threshold(),
            key_cooldown_sec: default_ranking_key_cooldown_sec(),
            key_stale_ttl_sec: default_ranking_key_stale_ttl_sec(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server: ServerSection {
                host: "127.0.0.1".to_string(),
                port: 3000,
                admin_enabled: default_admin_enabled(),
            },
            easyscholar: EasyScholarSection { keys: vec![] },
            ranking: RankingSection::default(),
            llm: LlmSection {
                default_provider: "".to_string(),
                enable_filter: false,
                strict_filter: false,
                providers: vec![],
                provider_configs: HashMap::new(),
            },
            search: SearchSection {
                default_ylo: Some(2020),
                enable_crossref: true,
                ss_limit: 100,
                oa_limit: 200,
                enabled_sources: vec![],
                openalex: SearchOpenAlexSection::default(),
                semanticscholar: SearchSemanticScholarSection::default(),
                arxiv: SearchArxivSection::default(),
                pubmed: SearchPubMedSection::default(),
                xrxiv: SearchXRxivSection::default(),
            },
        }
    }
}

impl LlmSection {
    /// Resolve provider order from config.
    ///
    /// Priority:
    /// 1) `llm.providers = ["..."]` (if non-empty)
    /// 2) default_provider first + discovered configured providers
    pub fn provider_order(&self) -> Vec<String> {
        if !self.providers.is_empty() {
            return dedup_keep_order(self.providers.clone());
        }

        let mut names = Vec::new();
        if !self.default_provider.trim().is_empty() {
            names.push(self.default_provider.to_lowercase());
        }

        let mut dynamic_names: Vec<String> = self
            .provider_configs
            .keys()
            .map(|k| k.to_lowercase())
            .collect();
        dynamic_names.sort();
        names.extend(dynamic_names);

        dedup_keep_order(names)
    }

    /// Resolve provider config by name from registry.
    pub fn resolve_provider_config(&self, name: &str) -> Option<ProviderConfig> {
        let normalized = name.to_lowercase();
        self.provider_configs.get(&normalized).cloned()
    }
}

fn dedup_keep_order(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values
        .into_iter()
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
        .filter(|v| seen.insert(v.clone()))
        .collect()
}

impl ServerConfig {
    /// Load configuration from `config.toml`
    pub fn load_from_file(path: &str) -> Result<Self> {
        let path = Path::new(path);
        
        if !path.exists() {
            warn!("Config file {} not found, using defaults", path.display());
            return Ok(Self::default());
        }

        let content = fs::read_to_string(path)
            .map_err(|e| GscholarError::Config(format!("Failed to read config file: {}", e)))?;

        let config: ServerConfig = toml::from_str(&content)
            .map_err(|e| GscholarError::Config(format!("Failed to parse config file: {}", e)))?;

        info!(
            host = %config.server.host,
            port = config.server.port,
            admin_enabled = config.server.admin_enabled,
            easyscholar_keys = config.easyscholar.keys.len(),
            ranking_queue_capacity = config.ranking.queue_capacity,
            ranking_min_chunk = config.ranking.min_chunk,
            ranking_max_chunk = config.ranking.max_chunk,
            ranking_max_concurrent_jobs = config.ranking.max_concurrent_jobs,
            ranking_scheduler_mode = %config.ranking.scheduler_mode,
            ranking_target_duration_sec = config.ranking.target_duration_sec,
            ranking_eta_scale = config.ranking.eta_scale,
            ranking_heartbeat_ms = config.ranking.heartbeat_ms,
            llm_enabled = config.llm.enable_filter,
            source_arxiv_max = config.search.arxiv.max_results,
            source_pubmed_max = config.search.pubmed.max_results,
            source_biorxiv_max = config.search.xrxiv.biorxiv_max_results,
            source_medrxiv_max = config.search.xrxiv.medrxiv_max_results,
            "Configuration loaded from config.toml"
        );

        Ok(config)
    }

    /// Get socket address for binding
    pub fn socket_addr(&self) -> std::result::Result<SocketAddr, std::net::AddrParseError> {
        format!("{}:{}", self.server.host, self.server.port).parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_config() {
        let toml = r#"
            [server]
            host = "127.0.0.1"
            port = 8080
            admin_enabled = true

            [easyscholar]
            keys = ["key1", "key2"]

            [ranking]
            queue_capacity = 64
            min_chunk = 4
            max_chunk = 20
            max_concurrent_jobs = 6
            scheduler_mode = "easy_backfill"
            target_duration_sec = 12
            eta_scale = 1.8
            heartbeat_ms = 150
            job_timeout_min_sec = 20
            key_fail_threshold = 4
            key_cooldown_sec = 20
            key_stale_ttl_sec = 120

            [llm]
            default_provider = "test"
            enable_filter = true
            strict_filter = false
            providers = ["bigmodel", "siliconflow"]
            
            [llm.registry.aiping]
            api_key = "abc"
            model = "gpt"
            endpoint = "https://www.aiping.cn/api/v1/chat/completions"

            [llm.registry.bigmodel]
            api_key = "bm-test"
            model = "GLM-4.7-Flash"
            endpoint = "https://open.bigmodel.cn/api/paas/v4/chat/completions"

            [search]
            default_ylo = 2023
            enable_crossref = false
            ss_limit = 50
            oa_limit = 50

            [search.openalex]
            max_results = 50
            timeout_sec = 20

            [search.semanticscholar]
            max_results = 50
            timeout_sec = 20

            [search.arxiv]
            max_results = 200
            page_size = 100
            sort_by = "relevance"
            sort_order = "descending"
            timeout_sec = 20
            request_delay_ms = 3000

            [search.pubmed]
            max_results = 200
            page_size = 100
            api_key = "pm-key"
            tool = "RscholarTest"
            email = "x@example.com"
            timeout_sec = 20
            delay_no_key_ms = 350
            delay_with_key_ms = 120

            [search.xrxiv]
            biorxiv_max_results = 180
            medrxiv_max_results = 170
            start_date = "2021-01-01"
            end_date = "2025-01-01"
            timeout_sec = 20
            request_delay_ms = 500
            max_retries = 5
        "#;

        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, "{}", toml).unwrap();

        let config = ServerConfig::load_from_file(file.path().to_str().unwrap()).unwrap();
        
        assert_eq!(config.server.port, 8080);
        assert!(config.server.admin_enabled);
        assert_eq!(config.easyscholar.keys.len(), 2);
        assert_eq!(config.ranking.queue_capacity, 64);
        assert_eq!(config.ranking.min_chunk, 4);
        assert_eq!(config.ranking.max_chunk, 20);
        assert_eq!(config.ranking.max_concurrent_jobs, 6);
        assert_eq!(config.ranking.scheduler_mode, "easy_backfill");
        assert_eq!(config.ranking.target_duration_sec, 12);
        assert!((config.ranking.eta_scale - 1.8).abs() < f64::EPSILON);
        assert_eq!(config.ranking.heartbeat_ms, 150);
        assert_eq!(config.ranking.job_timeout_min_sec, 20);
        assert_eq!(config.ranking.key_fail_threshold, 4);
        assert_eq!(config.ranking.key_cooldown_sec, 20);
        assert_eq!(config.ranking.key_stale_ttl_sec, 120);
        assert!(config.llm.enable_filter);
        assert_eq!(
            config.llm.provider_configs.get("aiping").map(|v| v.api_key.clone()),
            Some("abc".to_string())
        );
        assert_eq!(config.llm.providers, vec!["bigmodel", "siliconflow"]);
        assert_eq!(
            config
                .llm
                .provider_configs
                .get("bigmodel")
                .and_then(|v| v.endpoint.clone()),
            Some("https://open.bigmodel.cn/api/paas/v4/chat/completions".to_string())
        );
        assert_eq!(config.search.default_ylo, Some(2023));
        // All sections present → all enabled
        assert!(config.search.openalex.enabled);
        assert!(config.search.semanticscholar.enabled);
        assert!(config.search.arxiv.enabled);
        assert!(config.search.pubmed.enabled);
        assert!(config.search.xrxiv.enabled);
        assert_eq!(config.search.openalex.max_results, 50);
        assert_eq!(config.search.semanticscholar.max_results, 50);
        // effective_sources should derive from flags (enabled_sources is empty)
        let effective = config.search.effective_sources();
        assert!(effective.contains(&"openalex".to_string()));
        assert!(effective.contains(&"semanticscholar".to_string()));
        assert!(effective.contains(&"arxiv".to_string()));
        assert!(effective.contains(&"pubmed".to_string()));
        assert!(effective.contains(&"biorxiv".to_string()));
        assert!(effective.contains(&"medrxiv".to_string()));
        assert_eq!(config.search.arxiv.max_results, 200);
        assert_eq!(config.search.arxiv.page_size, 100);
        assert_eq!(config.search.pubmed.api_key, "pm-key");
        assert_eq!(config.search.pubmed.tool, "RscholarTest");
        assert_eq!(config.search.xrxiv.biorxiv_max_results, 180);
        assert_eq!(config.search.xrxiv.medrxiv_max_results, 170);
        assert_eq!(config.search.xrxiv.max_retries, 5);
    }

    #[test]
    fn test_llm_provider_order_and_resolve() {
        let mut section = LlmSection {
            default_provider: "siliconflow".to_string(),
            enable_filter: true,
            strict_filter: false,
            providers: vec![],
            provider_configs: HashMap::new(),
        };
        section.provider_configs.insert(
            "aiping".to_string(),
            ProviderConfig {
                api_key: "a".to_string(),
                model: "m1".to_string(),
                endpoint: None,
            },
        );
        section.provider_configs.insert(
            "siliconflow".to_string(),
            ProviderConfig {
                api_key: "b".to_string(),
                model: "m2".to_string(),
                endpoint: None,
            },
        );
        section.provider_configs.insert(
            "bigmodel".to_string(),
            ProviderConfig {
                api_key: "d".to_string(),
                model: "glm-4.7".to_string(),
                endpoint: None,
            },
        );
        section.provider_configs.insert(
            "custom".to_string(),
            ProviderConfig {
                api_key: "c".to_string(),
                model: "custom-model".to_string(),
                endpoint: Some("https://example.com/v1/chat/completions".to_string()),
            },
        );

        let order = section.provider_order();
        assert_eq!(order.first().map(|s| s.as_str()), Some("siliconflow"));
        assert!(order.contains(&"aiping".to_string()));
        assert!(order.contains(&"custom".to_string()));
        assert!(order.contains(&"bigmodel".to_string()));

        let custom_cfg = section.resolve_provider_config("custom").unwrap();
        assert_eq!(custom_cfg.model, "custom-model");
    }
}
