use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsEnvelope {
    pub version: u32,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsFieldError {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsMutationResponse {
    pub ok: bool,
    pub errors: Vec<SettingsFieldError>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsGetResponse {
    pub effective: Value,
    pub global: SettingsEnvelope,
    pub project: SettingsEnvelope,
    pub secrets_meta: Value,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsExportResponse {
    pub data: Value,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSetArgs {
    pub scope: String,
    pub patch: Value,
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResetArgs {
    pub scope: String,
    pub keys: Option<Vec<String>>,
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsExportArgs {
    pub project_root: Option<String>,
    pub include_project: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsImportArgs {
    pub json: Value,
    pub mode: String,
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsConnectivityTestArgs {
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConnectivityResult {
    pub provider: String,
    pub label: String,
    pub capability: String,
    pub endpoint: String,
    pub ok: bool,
    pub reachable: bool,
    pub compatibility: String,
    pub status: Option<u16>,
    pub latency_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedScope {
    pub(crate) envelope: SettingsEnvelope,
    pub(crate) warnings: Vec<String>,
    pub(crate) needs_write: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct CitationLlmRuntimeConfig {
    pub enabled: bool,
    pub model: String,
    pub endpoint: String,
    pub timeout_ms: u64,
    pub max_queries: usize,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CitationQueryEmbeddingRuntimeConfig {
    pub provider: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct CitationQueryExecutionRuntimeConfig {
    pub top_n: usize,
    pub mmr_lambda: f32,
    pub min_quality: f32,
    pub min_hit_ratio: f32,
    pub hit_score_threshold: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct CitationProviderRuntimeConfig {
    pub semantic_scholar_enabled: bool,
    pub semantic_scholar_api_key: Option<String>,
}

pub(crate) use agent_core::{
    AgentDomainConfig, AgentRuntimeConfig, AgentSamplingConfig, AgentSamplingProfilesConfig,
};
