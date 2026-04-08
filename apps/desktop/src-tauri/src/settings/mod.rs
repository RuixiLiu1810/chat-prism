mod connectivity;
pub(crate) mod keychain;
pub(crate) mod schema;
pub(crate) mod types;
mod validation;

// Re-export everything that was previously accessible as crate::settings::X
pub use types::ProviderConnectivityResult;
pub use types::SettingsConnectivityTestArgs;
pub use types::SettingsEnvelope;
pub use types::SettingsExportArgs;
pub use types::SettingsExportResponse;
pub use types::SettingsFieldError;
pub use types::SettingsGetResponse;
pub use types::SettingsImportArgs;
pub use types::SettingsMutationResponse;
pub use types::SettingsResetArgs;
pub use types::SettingsSetArgs;

pub(crate) use types::AgentRuntimeConfig;
pub(crate) use types::AgentDomainConfig;
pub(crate) use types::AgentSamplingConfig;
pub(crate) use types::AgentSamplingProfilesConfig;
pub(crate) use types::CitationLlmRuntimeConfig;
pub(crate) use types::CitationProviderRuntimeConfig;
pub(crate) use types::CitationQueryEmbeddingRuntimeConfig;
pub(crate) use types::CitationQueryExecutionRuntimeConfig;
pub(crate) use types::LoadedScope;

use reqwest::{Client, Method};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::Manager;

use connectivity::{classify_http_status, classify_runtime_probe_status};
use keychain::{
    migrate_secret_values_to_keychain, persist_secret_values_to_keychain_or_fallback,
    resolve_secret_value,
};
use schema::{
    as_object, default_global_envelope, default_project_envelope,
    default_secret_envelope, get_enum, get_in, get_string_or,
    is_canonical_envelope_data, migrate_global_envelope, migrate_project_envelope,
    migrate_secret_envelope, resolve_effective_settings, sanitize_global_settings,
    sanitize_project_settings, sanitize_secret_settings, to_secrets_meta,
};
use validation::{validate_global_patch, validate_project_patch, validate_secret_patch};

// ─── Constants ──────────────────────────────────────────────────────

const SETTINGS_SCHEMA_VERSION: u32 = 1;
const GLOBAL_SETTINGS_FILE_NAME: &str = "settings.json";
const SECRET_SETTINGS_FILE_NAME: &str = "secrets.json";
const PROJECT_SETTINGS_RELATIVE_PATH: &str = ".prism/config.json";
const KEYCHAIN_ACCOUNT: &str = "claude-prism";
const KEYCHAIN_SERVICE_AGENT_OPENAI: &str = "claude-prism.agent.openai.api-key";
const KEYCHAIN_SERVICE_SEMANTIC_SCHOLAR: &str = "claude-prism.semantic-scholar.api-key";
const KEYCHAIN_SERVICE_LLM_QUERY: &str = "claude-prism.llm-query.api-key";

// ─── From impl ──────────────────────────────────────────────────────

impl From<SettingsEnvelope> for LoadedScope {
    fn from(envelope: SettingsEnvelope) -> Self {
        Self {
            envelope,
            warnings: Vec::new(),
            needs_write: false,
        }
    }
}

// ─── Utility functions ──────────────────────────────────────────────

fn merge_value(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base_obj), Value::Object(patch_obj)) => {
            for (key, patch_value) in patch_obj {
                if let Some(base_value) = base_obj.get_mut(key) {
                    merge_value(base_value, patch_value);
                } else {
                    base_obj.insert(key.to_string(), patch_value.clone());
                }
            }
        }
        (base_value, patch_value) => {
            *base_value = patch_value.clone();
        }
    }
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Invalid path without parent: {}", path.display()))?;
    if !parent.exists() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }
    Ok(())
}

fn write_envelope(path: &Path, envelope: &SettingsEnvelope) -> Result<(), String> {
    ensure_parent_dir(path)?;
    let content = serde_json::to_string_pretty(envelope)
        .map_err(|e| format!("Failed to serialize settings JSON: {}", e))?;
    fs::write(path, format!("{}\n", content))
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

fn read_json_file(path: &Path) -> Result<Option<Value>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let parsed = serde_json::from_str::<Value>(&content)
        .map_err(|e| format!("Failed to parse {} as JSON: {}", path.display(), e))?;
    Ok(Some(parsed))
}

fn load_scope(
    path: &Path,
    scope_name: &str,
    migrate: fn(&Value) -> SettingsEnvelope,
    default_envelope: SettingsEnvelope,
) -> LoadedScope {
    if !path.exists() {
        return LoadedScope {
            envelope: default_envelope.clone(),
            warnings: Vec::new(),
            needs_write: false,
        };
    }

    match read_json_file(path) {
        Ok(Some(raw)) => {
            let migrated = migrate(&raw);
            LoadedScope {
                needs_write: !is_canonical_envelope_data(&raw, &migrated),
                envelope: migrated,
                warnings: Vec::new(),
            }
        }
        Ok(None) => LoadedScope {
            envelope: default_envelope.clone(),
            warnings: Vec::new(),
            needs_write: false,
        },
        Err(_) => LoadedScope {
            envelope: default_envelope,
            warnings: vec![format!(
                "{}: invalid JSON, fallback to defaults",
                scope_name
            )],
            needs_write: true,
        },
    }
}

// ─── File path functions ────────────────────────────────────────────

fn app_config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|e| format!("Failed to resolve app config dir: {}", e))
}

fn global_settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app_config_dir(app)?;
    Ok(dir.join(GLOBAL_SETTINGS_FILE_NAME))
}

fn secret_settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app_config_dir(app)?;
    Ok(dir.join(SECRET_SETTINGS_FILE_NAME))
}

fn project_settings_path(project_root: &str) -> PathBuf {
    Path::new(project_root).join(PROJECT_SETTINGS_RELATIVE_PATH)
}

fn scope_file_path(
    app: &tauri::AppHandle,
    scope: &str,
    project_root: Option<&str>,
) -> Result<PathBuf, String> {
    match scope {
        "global" => global_settings_path(app),
        "secret" => secret_settings_path(app),
        "project" => {
            let root = project_root
                .ok_or_else(|| "projectRoot is required when scope is project".to_string())?;
            Ok(project_settings_path(root))
        }
        _ => Err("scope must be one of: global, project, secret".to_string()),
    }
}

fn load_scope_for_path(path: &Path, scope: &str) -> LoadedScope {
    load_scope(
        path,
        scope,
        migrate_for_scope(scope),
        default_envelope_for_scope(scope),
    )
}

fn persist_loaded_if_needed(
    path: &Path,
    loaded: &LoadedScope,
    scope: &str,
    warnings: &mut Vec<String>,
) {
    if loaded.needs_write && path.exists() {
        if let Err(err) = write_envelope(path, &loaded.envelope) {
            warnings.push(format!("{}: failed to persist migration ({})", scope, err));
        }
    }
}

fn remove_json_path(root: &mut Value, path: &str) -> Result<(), String> {
    let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err("empty path".to_string());
    }

    fn remove_rec(value: &mut Value, parts: &[&str]) {
        if parts.is_empty() {
            return;
        }
        if parts.len() == 1 {
            if let Some(obj) = value.as_object_mut() {
                obj.remove(parts[0]);
            }
            return;
        }
        if let Some(obj) = value.as_object_mut() {
            if let Some(next) = obj.get_mut(parts[0]) {
                remove_rec(next, &parts[1..]);
            }
        }
    }

    remove_rec(root, &parts);
    Ok(())
}

fn extract_import_payload(input: &Value) -> (Option<Value>, Option<Value>, Vec<String>) {
    let mut warnings = Vec::<String>::new();
    if let Some(obj) = as_object(input) {
        let global = obj.get("global").map(|v| {
            if let Some(inner) = get_in(v, &["data"]) {
                inner.clone()
            } else {
                v.clone()
            }
        });
        let project = obj.get("project").map(|v| {
            if let Some(inner) = get_in(v, &["data"]) {
                inner.clone()
            } else {
                v.clone()
            }
        });
        if global.is_some() || project.is_some() {
            return (global, project, warnings);
        }
    }

    warnings.push(
        "import payload has no top-level global/project; treated as global payload".to_string(),
    );
    (Some(input.clone()), None, warnings)
}

// ─── Scope router functions ─────────────────────────────────────────

fn validate_patch_for_scope(scope: &str, patch: &Value) -> Vec<SettingsFieldError> {
    match scope {
        "global" => validate_global_patch(patch),
        "project" => validate_project_patch(patch),
        "secret" => validate_secret_patch(patch),
        _ => vec![SettingsFieldError {
            path: "scope".to_string(),
            message: "must be one of: global, project, secret".to_string(),
        }],
    }
}

fn sanitize_for_scope(scope: &str, value: &Value) -> Value {
    match scope {
        "global" => sanitize_global_settings(value),
        "project" => sanitize_project_settings(value),
        "secret" => sanitize_secret_settings(value),
        _ => value.clone(),
    }
}

fn default_envelope_for_scope(scope: &str) -> SettingsEnvelope {
    match scope {
        "global" => default_global_envelope(),
        "project" => default_project_envelope(),
        "secret" => default_secret_envelope(),
        _ => default_global_envelope(),
    }
}

fn migrate_for_scope(scope: &str) -> fn(&Value) -> SettingsEnvelope {
    match scope {
        "global" => migrate_global_envelope,
        "project" => migrate_project_envelope,
        "secret" => migrate_secret_envelope,
        _ => migrate_global_envelope,
    }
}

// ─── Public runtime loaders (used by other modules) ─────────────────

pub(crate) fn load_agent_runtime(
    app: &tauri::AppHandle,
    project_root: Option<&str>,
) -> Result<AgentRuntimeConfig, String> {
    let global_path = global_settings_path(app)?;
    let secret_path = secret_settings_path(app)?;
    let global_loaded = load_scope_for_path(&global_path, "global");
    let secret_loaded = load_scope_for_path(&secret_path, "secret");
    let project_loaded = if let Some(root) = project_root {
        let project_path = project_settings_path(root);
        load_scope_for_path(&project_path, "project")
    } else {
        default_project_envelope().into()
    };

    let effective =
        resolve_effective_settings(&global_loaded.envelope.data, &project_loaded.envelope.data);
    let runtime = get_enum(
        get_in(&effective, &["integrations", "agent", "runtime"]),
        &["claude_cli", "local_agent"],
        "claude_cli",
    );
    let provider = get_enum(
        get_in(&effective, &["integrations", "agent", "provider"]),
        &["openai", "minimax", "deepseek"],
        "openai",
    );
    let model = get_string_or(
        get_in(&effective, &["integrations", "agent", "model"]),
        "gpt-5.4",
    );
    let base_url = get_string_or(
        get_in(&effective, &["integrations", "agent", "baseUrl"]),
        "https://api.openai.com/v1",
    );
    let domain = get_enum(
        get_in(&effective, &["integrations", "agent", "domainConfig", "domain"]),
        &["general", "biomedical", "chemistry", "custom"],
        "general",
    );
    let terminology_strictness = get_enum(
        get_in(
            &effective,
            &[
                "integrations",
                "agent",
                "domainConfig",
                "terminologyStrictness",
            ],
        ),
        &["strict", "moderate", "relaxed"],
        "moderate",
    );
    let custom_instructions = get_in(
        &effective,
        &[
            "integrations",
            "agent",
            "domainConfig",
            "customInstructions",
        ],
    )
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(str::to_string);
    let api_key = resolve_secret_value(
        &secret_loaded.envelope.data,
        &["integrations", "agent", "apiKey"],
        KEYCHAIN_SERVICE_AGENT_OPENAI,
    );
    let read_sampling = |name: &str, fallback: (f64, f64, u32)| AgentSamplingConfig {
        temperature: get_in(
            &effective,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                name,
                "temperature",
            ],
        )
        .and_then(Value::as_f64)
        .unwrap_or(fallback.0),
        top_p: get_in(
            &effective,
            &["integrations", "agent", "samplingProfiles", name, "topP"],
        )
        .and_then(Value::as_f64)
        .unwrap_or(fallback.1),
        max_tokens: get_in(
            &effective,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                name,
                "maxTokens",
            ],
        )
        .and_then(Value::as_u64)
        .map(|value| value as u32)
        .unwrap_or(fallback.2),
    };

    Ok(AgentRuntimeConfig {
        runtime,
        provider,
        model,
        base_url,
        api_key,
        domain_config: AgentDomainConfig {
            domain,
            custom_instructions,
            terminology_strictness,
        },
        sampling_profiles: AgentSamplingProfilesConfig {
            edit_stable: read_sampling("editStable", (0.2, 0.9, 8192)),
            analysis_balanced: read_sampling("analysisBalanced", (0.4, 0.9, 6144)),
            analysis_deep: read_sampling("analysisDeep", (0.3, 0.92, 12288)),
            chat_flexible: read_sampling("chatFlexible", (0.7, 0.95, 4096)),
        },
    })
}

pub(crate) fn load_citation_llm_runtime(
    app: &tauri::AppHandle,
    project_root: Option<&str>,
) -> Result<CitationLlmRuntimeConfig, String> {
    let global_path = global_settings_path(app)?;
    let secret_path = secret_settings_path(app)?;
    let global_loaded = load_scope_for_path(&global_path, "global");
    let secret_loaded = load_scope_for_path(&secret_path, "secret");
    let project_loaded = if let Some(root) = project_root {
        let project_path = project_settings_path(root);
        load_scope_for_path(&project_path, "project")
    } else {
        default_project_envelope().into()
    };

    let effective =
        resolve_effective_settings(&global_loaded.envelope.data, &project_loaded.envelope.data);
    let enabled = get_in(&effective, &["citation", "search", "llmQuery", "enabled"])
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let model = get_string_or(
        get_in(&effective, &["citation", "search", "llmQuery", "model"]),
        "gpt-4o-mini",
    );
    let endpoint = get_string_or(
        get_in(&effective, &["citation", "search", "llmQuery", "endpoint"]),
        "https://api.openai.com/v1/chat/completions",
    );
    let timeout_ms = get_in(&effective, &["citation", "search", "llmQuery", "timeoutMs"])
        .and_then(Value::as_u64)
        .unwrap_or(6000)
        .clamp(2000, 20000);
    let max_queries = get_in(
        &effective,
        &["citation", "search", "llmQuery", "maxQueries"],
    )
    .and_then(Value::as_u64)
    .unwrap_or(3)
    .clamp(1, 6) as usize;
    let api_key = resolve_secret_value(
        &secret_loaded.envelope.data,
        &["integrations", "llmQuery", "apiKey"],
        KEYCHAIN_SERVICE_LLM_QUERY,
    );

    Ok(CitationLlmRuntimeConfig {
        enabled,
        model,
        endpoint,
        timeout_ms,
        max_queries,
        api_key,
    })
}

pub(crate) fn load_citation_query_embedding_runtime(
    app: &tauri::AppHandle,
    project_root: Option<&str>,
) -> Result<CitationQueryEmbeddingRuntimeConfig, String> {
    let global_path = global_settings_path(app)?;
    let global_loaded = load_scope_for_path(&global_path, "global");
    let project_loaded = if let Some(root) = project_root {
        let project_path = project_settings_path(root);
        load_scope_for_path(&project_path, "project")
    } else {
        default_project_envelope().into()
    };
    let effective =
        resolve_effective_settings(&global_loaded.envelope.data, &project_loaded.envelope.data);

    let provider = get_enum(
        get_in(
            &effective,
            &["citation", "search", "queryEmbedding", "provider"],
        ),
        &["none", "local_embedding"],
        "none",
    );
    let timeout_ms = get_in(
        &effective,
        &["citation", "search", "queryEmbedding", "timeoutMs"],
    )
    .and_then(Value::as_u64)
    .unwrap_or(1200)
    .clamp(100, 10000);

    Ok(CitationQueryEmbeddingRuntimeConfig {
        provider,
        timeout_ms,
    })
}

pub(crate) fn load_citation_query_execution_runtime(
    app: &tauri::AppHandle,
    project_root: Option<&str>,
) -> Result<CitationQueryExecutionRuntimeConfig, String> {
    let global_path = global_settings_path(app)?;
    let global_loaded = load_scope_for_path(&global_path, "global");
    let project_loaded = if let Some(root) = project_root {
        let project_path = project_settings_path(root);
        load_scope_for_path(&project_path, "project")
    } else {
        default_project_envelope().into()
    };
    let effective =
        resolve_effective_settings(&global_loaded.envelope.data, &project_loaded.envelope.data);

    let top_n = get_in(
        &effective,
        &["citation", "search", "queryExecution", "topN"],
    )
    .and_then(Value::as_u64)
    .unwrap_or(5)
    .clamp(1, 10) as usize;
    let mmr_lambda = get_in(
        &effective,
        &["citation", "search", "queryExecution", "mmrLambda"],
    )
    .and_then(Value::as_f64)
    .unwrap_or(0.72)
    .clamp(0.0, 1.0) as f32;
    let min_quality = get_in(
        &effective,
        &["citation", "search", "queryExecution", "minQuality"],
    )
    .and_then(Value::as_f64)
    .unwrap_or(0.24)
    .clamp(0.0, 1.0) as f32;
    let min_hit_ratio = get_in(
        &effective,
        &["citation", "search", "queryExecution", "minHitRatio"],
    )
    .and_then(Value::as_f64)
    .unwrap_or(0.45)
    .clamp(0.0, 1.0) as f32;
    let hit_score_threshold = get_in(
        &effective,
        &["citation", "search", "queryExecution", "hitScoreThreshold"],
    )
    .and_then(Value::as_f64)
    .unwrap_or(0.58)
    .clamp(0.0, 1.0) as f32;

    Ok(CitationQueryExecutionRuntimeConfig {
        top_n,
        mmr_lambda,
        min_quality,
        min_hit_ratio,
        hit_score_threshold,
    })
}

pub(crate) fn load_citation_provider_runtime(
    app: &tauri::AppHandle,
    project_root: Option<&str>,
) -> Result<CitationProviderRuntimeConfig, String> {
    let global_path = global_settings_path(app)?;
    let secret_path = secret_settings_path(app)?;
    let global_loaded = load_scope_for_path(&global_path, "global");
    let secret_loaded = load_scope_for_path(&secret_path, "secret");
    let project_loaded = if let Some(root) = project_root {
        let project_path = project_settings_path(root);
        load_scope_for_path(&project_path, "project")
    } else {
        default_project_envelope().into()
    };
    let effective =
        resolve_effective_settings(&global_loaded.envelope.data, &project_loaded.envelope.data);

    let semantic_scholar_enabled =
        get_in(&effective, &["integrations", "semanticScholar", "enabled"])
            .and_then(Value::as_bool)
            .unwrap_or(true);
    let semantic_scholar_api_key = resolve_secret_value(
        &secret_loaded.envelope.data,
        &["integrations", "semanticScholar", "apiKey"],
        KEYCHAIN_SERVICE_SEMANTIC_SCHOLAR,
    );

    Ok(CitationProviderRuntimeConfig {
        semantic_scholar_enabled,
        semantic_scholar_api_key,
    })
}

// ─── HTTP probe helpers ─────────────────────────────────────────────

async fn probe_http_endpoint(
    client: &Client,
    provider: &str,
    label: &str,
    capability: &str,
    endpoint: String,
    method: Method,
    headers: Vec<(&'static str, String)>,
    body: Option<String>,
) -> ProviderConnectivityResult {
    let started = Instant::now();
    let mut request = client.request(method, &endpoint);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    if let Some(payload) = body {
        request = request.body(payload);
    }

    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let (ok, reachable, compatibility, message) = classify_http_status(status);
            ProviderConnectivityResult {
                provider: provider.to_string(),
                label: label.to_string(),
                capability: capability.to_string(),
                endpoint,
                ok,
                reachable,
                compatibility,
                status: Some(status),
                latency_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                message,
            }
        }
        Err(err) => {
            let message = if err.is_timeout() {
                "Connection timed out.".to_string()
            } else if err.is_connect() {
                "Connection failed.".to_string()
            } else {
                format!("Request failed: {}", err)
            };
            ProviderConnectivityResult {
                provider: provider.to_string(),
                label: label.to_string(),
                capability: capability.to_string(),
                endpoint,
                ok: false,
                reachable: false,
                compatibility: "unknown".to_string(),
                status: None,
                latency_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                message,
            }
        }
    }
}

async fn probe_runtime_endpoint(
    client: &Client,
    provider: &str,
    label: &str,
    capability: &str,
    endpoint: String,
    headers: Vec<(&'static str, String)>,
    body: String,
) -> ProviderConnectivityResult {
    let started = Instant::now();
    let mut request = client
        .request(Method::POST, &endpoint)
        .header("content-type", "application/json");
    for (name, value) in headers {
        request = request.header(name, value);
    }
    request = request.body(body);

    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let (ok, reachable, compatibility, message) =
                classify_runtime_probe_status(status, capability);
            ProviderConnectivityResult {
                provider: provider.to_string(),
                label: label.to_string(),
                capability: capability.to_string(),
                endpoint,
                ok,
                reachable,
                compatibility,
                status: Some(status),
                latency_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                message,
            }
        }
        Err(err) => {
            let message = if err.is_timeout() {
                "Connection timed out.".to_string()
            } else if err.is_connect() {
                "Connection failed.".to_string()
            } else {
                format!("Request failed: {}", err)
            };
            ProviderConnectivityResult {
                provider: provider.to_string(),
                label: label.to_string(),
                capability: capability.to_string(),
                endpoint,
                ok: false,
                reachable: false,
                compatibility: "unknown".to_string(),
                status: None,
                latency_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                message,
            }
        }
    }
}

// ─── Tauri commands ─────────────────────────────────────────────────

#[tauri::command]
pub async fn settings_test_provider_connectivity(
    app: tauri::AppHandle,
    args: Option<SettingsConnectivityTestArgs>,
) -> Result<Vec<ProviderConnectivityResult>, String> {
    let project_root = args
        .as_ref()
        .and_then(|a| a.project_root.as_deref())
        .map(ToString::to_string);

    let global_path = global_settings_path(&app)?;
    let secret_path = secret_settings_path(&app)?;
    let global_loaded = load_scope_for_path(&global_path, "global");
    let secret_loaded = load_scope_for_path(&secret_path, "secret");
    let project_loaded = if let Some(root) = project_root.as_deref() {
        let project_path = project_settings_path(root);
        load_scope_for_path(&project_path, "project")
    } else {
        default_project_envelope().into()
    };
    let effective =
        resolve_effective_settings(&global_loaded.envelope.data, &project_loaded.envelope.data);

    let agent_api_key = resolve_secret_value(
        &secret_loaded.envelope.data,
        &["integrations", "agent", "apiKey"],
        KEYCHAIN_SERVICE_AGENT_OPENAI,
    );
    let agent_base_url = get_string_or(
        get_in(&effective, &["integrations", "agent", "baseUrl"]),
        "https://api.openai.com/v1",
    );
    let semantic_api_key = resolve_secret_value(
        &secret_loaded.envelope.data,
        &["integrations", "semanticScholar", "apiKey"],
        KEYCHAIN_SERVICE_SEMANTIC_SCHOLAR,
    );
    let llm_api_key = resolve_secret_value(
        &secret_loaded.envelope.data,
        &["integrations", "llmQuery", "apiKey"],
        KEYCHAIN_SERVICE_LLM_QUERY,
    );
    let llm_endpoint = get_string_or(
        get_in(&effective, &["citation", "search", "llmQuery", "endpoint"]),
        "https://api.openai.com/v1/chat/completions",
    );

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| format!("failed to build connectivity client: {}", e))?;

    let mut results = Vec::<ProviderConnectivityResult>::new();

    let mut agent_headers = vec![("accept", "application/json".to_string())];
    if let Some(key) = agent_api_key {
        agent_headers.push(("authorization", format!("Bearer {}", key)));
    }
    results.push(
        probe_http_endpoint(
            &client,
            "agentReachability",
            "Agent Base URL",
            "models",
            format!("{}/models", agent_base_url.trim_end_matches('/')),
            Method::GET,
            agent_headers,
            None,
        )
        .await,
    );
    let mut runtime_headers = vec![("accept", "application/json".to_string())];
    if let Some(key) = resolve_secret_value(
        &secret_loaded.envelope.data,
        &["integrations", "agent", "apiKey"],
        KEYCHAIN_SERVICE_AGENT_OPENAI,
    ) {
        runtime_headers.push(("authorization", format!("Bearer {}", key)));
    }
    results.push(
        probe_runtime_endpoint(
            &client,
            "agentResponses",
            "Agent Runtime (Responses)",
            "responses",
            format!("{}/responses", agent_base_url.trim_end_matches('/')),
            runtime_headers.clone(),
            "{}".to_string(),
        )
        .await,
    );
    results.push(
        probe_runtime_endpoint(
            &client,
            "agentChatCompletions",
            "Agent Runtime (Chat Completions)",
            "chat_completions",
            format!("{}/chat/completions", agent_base_url.trim_end_matches('/')),
            runtime_headers,
            "{}".to_string(),
        )
        .await,
    );

    let mut semantic_headers = vec![("accept", "application/json".to_string())];
    if let Some(key) = semantic_api_key {
        semantic_headers.push(("x-api-key", key));
    }
    results.push(
        probe_http_endpoint(
            &client,
            "semanticScholar",
            "Semantic Scholar",
            "search",
            "https://api.semanticscholar.org/graph/v1/paper/search?query=graphene&limit=1&fields=title".to_string(),
            Method::GET,
            semantic_headers,
            None,
        )
        .await,
    );

    results.push(
        probe_http_endpoint(
            &client,
            "openAlex",
            "OpenAlex",
            "search",
            "https://api.openalex.org/works?search=graphene&per-page=1".to_string(),
            Method::GET,
            vec![("accept", "application/json".to_string())],
            None,
        )
        .await,
    );

    results.push(
        probe_http_endpoint(
            &client,
            "crossref",
            "Crossref",
            "search",
            "https://api.crossref.org/works?query=graphene&rows=1".to_string(),
            Method::GET,
            vec![
                ("accept", "application/json".to_string()),
                (
                    "user-agent",
                    "ClaudePrism/1.1.0 (mailto:support@example.com)".to_string(),
                ),
            ],
            None,
        )
        .await,
    );

    let mut llm_headers = vec![("accept", "application/json".to_string())];
    if let Some(key) = llm_api_key {
        llm_headers.push(("authorization", format!("Bearer {}", key)));
    }
    results.push(
        probe_http_endpoint(
            &client,
            "llmQuery",
            "LLM Endpoint",
            "llm",
            llm_endpoint,
            Method::GET,
            llm_headers,
            None,
        )
        .await,
    );

    Ok(results)
}

#[tauri::command]
pub fn settings_get(
    app: tauri::AppHandle,
    project_root: Option<String>,
) -> Result<SettingsGetResponse, String> {
    let global_path = global_settings_path(&app)?;
    let secret_path = secret_settings_path(&app)?;

    let mut warnings = Vec::<String>::new();

    let global_loaded = load_scope_for_path(&global_path, "global");
    warnings.extend(global_loaded.warnings.clone());
    persist_loaded_if_needed(&global_path, &global_loaded, "global", &mut warnings);

    let mut secret_loaded = load_scope_for_path(&secret_path, "secret");
    if migrate_secret_values_to_keychain(&mut secret_loaded.envelope.data, &mut warnings) {
        secret_loaded.needs_write = true;
    }
    warnings.extend(secret_loaded.warnings.clone());
    persist_loaded_if_needed(&secret_path, &secret_loaded, "secret", &mut warnings);

    let project_loaded = if let Some(root) = project_root.as_deref() {
        let project_path = project_settings_path(root);
        let loaded = load_scope_for_path(&project_path, "project");
        warnings.extend(loaded.warnings.clone());
        persist_loaded_if_needed(&project_path, &loaded, "project", &mut warnings);
        loaded
    } else {
        default_project_envelope().into()
    };

    let effective =
        resolve_effective_settings(&global_loaded.envelope.data, &project_loaded.envelope.data);
    let secrets_meta = to_secrets_meta(&secret_loaded.envelope.data);

    Ok(SettingsGetResponse {
        effective,
        global: global_loaded.envelope,
        project: project_loaded.envelope,
        secrets_meta,
        warnings,
    })
}

#[tauri::command]
pub fn settings_set(
    app: tauri::AppHandle,
    args: SettingsSetArgs,
) -> Result<SettingsMutationResponse, String> {
    let scope = args.scope.as_str();
    let path = scope_file_path(&app, scope, args.project_root.as_deref())?;

    let errors = validate_patch_for_scope(scope, &args.patch);
    if !errors.is_empty() {
        return Ok(SettingsMutationResponse {
            ok: false,
            errors,
            warnings: Vec::new(),
        });
    }

    let loaded = load_scope_for_path(&path, scope);
    let mut warnings = loaded.warnings.clone();
    let mut merged = loaded.envelope.data.clone();
    merge_value(&mut merged, &args.patch);
    let sanitized = sanitize_for_scope(scope, &merged);
    let mut next = SettingsEnvelope {
        version: SETTINGS_SCHEMA_VERSION,
        data: sanitized,
    };
    if scope == "secret" {
        persist_secret_values_to_keychain_or_fallback(&mut next.data, &mut warnings);
    }
    write_envelope(&path, &next)?;

    if loaded.needs_write {
        warnings.push(format!(
            "{}: existing file was normalized before write",
            scope
        ));
    }

    Ok(SettingsMutationResponse {
        ok: true,
        errors: Vec::new(),
        warnings,
    })
}

#[tauri::command]
pub fn settings_reset(
    app: tauri::AppHandle,
    args: SettingsResetArgs,
) -> Result<SettingsMutationResponse, String> {
    let scope = args.scope.as_str();
    let path = scope_file_path(&app, scope, args.project_root.as_deref())?;
    let loaded = load_scope_for_path(&path, scope);
    let mut warnings = loaded.warnings.clone();
    let mut next_data = loaded.envelope.data.clone();

    match args.keys {
        None => {
            next_data = default_envelope_for_scope(scope).data;
        }
        Some(keys) if keys.is_empty() => {
            next_data = default_envelope_for_scope(scope).data;
        }
        Some(keys) => {
            for key in &keys {
                if let Err(err) = remove_json_path(&mut next_data, key) {
                    warnings.push(format!("invalid reset key '{}': {}", key, err));
                }
            }
            next_data = sanitize_for_scope(scope, &next_data);
        }
    }

    if scope == "secret" {
        persist_secret_values_to_keychain_or_fallback(&mut next_data, &mut warnings);
    }

    let next = SettingsEnvelope {
        version: SETTINGS_SCHEMA_VERSION,
        data: next_data,
    };
    write_envelope(&path, &next)?;

    Ok(SettingsMutationResponse {
        ok: true,
        errors: Vec::new(),
        warnings,
    })
}

#[tauri::command]
pub fn settings_export(
    app: tauri::AppHandle,
    args: Option<SettingsExportArgs>,
) -> Result<SettingsExportResponse, String> {
    let include_project = args
        .as_ref()
        .and_then(|a| a.include_project)
        .unwrap_or(false);
    let project_root = args
        .as_ref()
        .and_then(|a| a.project_root.as_ref())
        .map(ToString::to_string);

    let global_path = global_settings_path(&app)?;
    let global_loaded = load_scope_for_path(&global_path, "global");
    let mut warnings = global_loaded.warnings.clone();

    let mut data = json!({
      "version": SETTINGS_SCHEMA_VERSION,
      "global": global_loaded.envelope.data
    });

    if include_project {
        if let Some(root) = project_root {
            let project_path = project_settings_path(&root);
            let project_loaded = load_scope_for_path(&project_path, "project");
            warnings.extend(project_loaded.warnings.clone());
            if let Some(obj) = data.as_object_mut() {
                obj.insert("project".to_string(), project_loaded.envelope.data);
            }
        } else {
            warnings.push(
                "settings_export: includeProject=true but projectRoot is missing; project omitted"
                    .to_string(),
            );
        }
    }

    Ok(SettingsExportResponse { data, warnings })
}

#[tauri::command]
pub fn settings_import(
    app: tauri::AppHandle,
    args: SettingsImportArgs,
) -> Result<SettingsMutationResponse, String> {
    let mode = args.mode.to_lowercase();
    if mode != "merge" && mode != "replace" {
        return Ok(SettingsMutationResponse {
            ok: false,
            errors: vec![SettingsFieldError {
                path: "mode".to_string(),
                message: "must be 'merge' or 'replace'".to_string(),
            }],
            warnings: Vec::new(),
        });
    }

    let (global_import, project_import, mut warnings) = extract_import_payload(&args.json);

    let mut all_errors = Vec::<SettingsFieldError>::new();
    if let Some(ref g) = global_import {
        all_errors.extend(validate_global_patch(g));
    }
    if let Some(ref p) = project_import {
        all_errors.extend(validate_project_patch(p));
    }
    if !all_errors.is_empty() {
        return Ok(SettingsMutationResponse {
            ok: false,
            errors: all_errors,
            warnings,
        });
    }

    if let Some(g_import) = global_import {
        let global_path = global_settings_path(&app)?;
        let loaded = load_scope_for_path(&global_path, "global");
        warnings.extend(loaded.warnings.clone());
        let next_data = if mode == "replace" {
            sanitize_global_settings(&g_import)
        } else {
            let mut merged = loaded.envelope.data.clone();
            merge_value(&mut merged, &g_import);
            sanitize_global_settings(&merged)
        };
        let next = SettingsEnvelope {
            version: SETTINGS_SCHEMA_VERSION,
            data: next_data,
        };
        write_envelope(&global_path, &next)?;
    }

    if let Some(p_import) = project_import {
        if let Some(root) = args.project_root.as_deref() {
            let project_path = project_settings_path(root);
            let loaded = load_scope_for_path(&project_path, "project");
            warnings.extend(loaded.warnings.clone());
            let next_data = if mode == "replace" {
                sanitize_project_settings(&p_import)
            } else {
                let mut merged = loaded.envelope.data.clone();
                merge_value(&mut merged, &p_import);
                sanitize_project_settings(&merged)
            };
            let next = SettingsEnvelope {
                version: SETTINGS_SCHEMA_VERSION,
                data: next_data,
            };
            write_envelope(&project_path, &next)?;
        } else {
            warnings.push(
                "settings_import: project payload detected but projectRoot missing; skipped project import"
                    .to_string(),
            );
        }
    }

    Ok(SettingsMutationResponse {
        ok: true,
        errors: Vec::new(),
        warnings,
    })
}
