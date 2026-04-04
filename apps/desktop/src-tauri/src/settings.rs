use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::Manager;

const SETTINGS_SCHEMA_VERSION: u32 = 1;
const GLOBAL_SETTINGS_FILE_NAME: &str = "settings.json";
const SECRET_SETTINGS_FILE_NAME: &str = "secrets.json";
const PROJECT_SETTINGS_RELATIVE_PATH: &str = ".prism/config.json";
const KEYCHAIN_ACCOUNT: &str = "claude-prism";
const KEYCHAIN_SERVICE_AGENT_OPENAI: &str = "claude-prism.agent.openai.api-key";
const KEYCHAIN_SERVICE_SEMANTIC_SCHOLAR: &str = "claude-prism.semantic-scholar.api-key";
const KEYCHAIN_SERVICE_LLM_QUERY: &str = "claude-prism.llm-query.api-key";

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
struct LoadedScope {
    envelope: SettingsEnvelope,
    warnings: Vec<String>,
    needs_write: bool,
}

fn default_global_settings() -> Value {
    json!({
      "general": {
        "theme": "system",
        "language": "zh-CN",
        "openInEditor": {
          "defaultEditor": "system"
        }
      },
      "citation": {
        "stylePolicy": "auto",
        "autoApplyThreshold": 0.64,
        "reviewThreshold": 0.5,
        "search": {
          "limit": 8,
          "llmQuery": {
            "enabled": false,
            "model": "gpt-4o-mini",
            "endpoint": "https://api.openai.com/v1/chat/completions",
            "timeoutMs": 6000,
            "maxQueries": 3
          },
          "queryEmbedding": {
            "provider": "none",
            "timeoutMs": 1200
          },
          "queryExecution": {
            "topN": 5,
            "mmrLambda": 0.72,
            "minQuality": 0.24,
            "minHitRatio": 0.45,
            "hitScoreThreshold": 0.58
          }
        }
      },
      "integrations": {
        "agent": {
          "runtime": "claude_cli",
          "provider": "openai",
          "model": "gpt-5.4",
          "baseUrl": "https://api.openai.com/v1",
          "samplingProfiles": {
            "editStable": {
              "temperature": 0.2,
              "topP": 0.9,
              "maxTokens": 8192
            },
            "analysisBalanced": {
              "temperature": 0.4,
              "topP": 0.9,
              "maxTokens": 6144
            },
            "analysisDeep": {
              "temperature": 0.3,
              "topP": 0.92,
              "maxTokens": 12288
            },
            "chatFlexible": {
              "temperature": 0.7,
              "topP": 0.95,
              "maxTokens": 4096
            }
          }
        },
        "semanticScholar": {
          "enabled": true
        },
        "zotero": {
          "autoSyncOnApply": true
        }
      },
      "advanced": {
        "debugEnabled": false,
        "logLevel": "info"
      }
    })
}

fn default_project_settings() -> Value {
    json!({})
}

fn default_secret_settings() -> Value {
    json!({
      "integrations": {
        "agent": {
          "apiKey": null
        },
        "semanticScholar": {
          "apiKey": null
        },
        "llmQuery": {
          "apiKey": null
        }
      }
    })
}

fn default_global_envelope() -> SettingsEnvelope {
    SettingsEnvelope {
        version: SETTINGS_SCHEMA_VERSION,
        data: sanitize_global_settings(&default_global_settings()),
    }
}

fn default_project_envelope() -> SettingsEnvelope {
    SettingsEnvelope {
        version: SETTINGS_SCHEMA_VERSION,
        data: sanitize_project_settings(&default_project_settings()),
    }
}

fn default_secret_envelope() -> SettingsEnvelope {
    SettingsEnvelope {
        version: SETTINGS_SCHEMA_VERSION,
        data: sanitize_secret_settings(&default_secret_settings()),
    }
}

fn as_object(value: &Value) -> Option<&Map<String, Value>> {
    match value {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

fn get_in<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        let obj = as_object(current)?;
        current = obj.get(*segment)?;
    }
    Some(current)
}

fn get_number_in_range(value: Option<&Value>, min: f64, max: f64) -> Option<f64> {
    match value.and_then(Value::as_f64) {
        Some(num) if num.is_finite() => Some(num.clamp(min, max)),
        _ => None,
    }
}

fn get_number_in_range_strict(value: Option<&Value>, min: f64, max: f64) -> Option<f64> {
    match value.and_then(Value::as_f64) {
        Some(num) if num.is_finite() && num >= min && num <= max => Some(num),
        _ => None,
    }
}

fn get_bool(value: Option<&Value>, fallback: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(fallback)
}

fn get_enum(value: Option<&Value>, allowed: &[&str], fallback: &str) -> String {
    if let Some(s) = value.and_then(Value::as_str) {
        if allowed.contains(&s) {
            return s.to_string();
        }
    }
    fallback.to_string()
}

fn get_string_or_null(value: Option<&Value>) -> Value {
    match value.and_then(Value::as_str) {
        Some(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Value::Null
            } else {
                Value::String(trimmed.to_string())
            }
        }
        None => Value::Null,
    }
}

fn get_string_or(value: Option<&Value>, fallback: &str) -> String {
    match value.and_then(Value::as_str) {
        Some(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                fallback.to_string()
            } else {
                trimmed.to_string()
            }
        }
        None => fallback.to_string(),
    }
}

fn normalize_threshold_pair(auto_apply: f64, review: f64) -> (f64, f64) {
    let auto = auto_apply.clamp(0.0, 1.0);
    let rev = review.clamp(0.0, 1.0).min(auto);
    (auto, rev)
}

fn legacy_global_data(raw: &Value) -> Value {
    let mut out = Map::<String, Value>::new();
    let root = as_object(raw);

    if let Some(theme) = root.and_then(|r| r.get("theme")).and_then(Value::as_str) {
        out.insert("general".to_string(), json!({ "theme": theme }));
    }
    if let Some(language) = root.and_then(|r| r.get("language")).and_then(Value::as_str) {
        let general = out
            .entry("general".to_string())
            .or_insert_with(|| json!({}));
        if let Some(obj) = general.as_object_mut() {
            obj.insert("language".to_string(), Value::String(language.to_string()));
        }
    }
    if let Some(editor) = root
        .and_then(|r| r.get("defaultEditor"))
        .and_then(Value::as_str)
    {
        let general = out
            .entry("general".to_string())
            .or_insert_with(|| json!({}));
        if let Some(obj) = general.as_object_mut() {
            obj.insert(
                "openInEditor".to_string(),
                json!({ "defaultEditor": editor }),
            );
        }
    }

    if let Some(debug_enabled) = root
        .and_then(|r| r.get("debugEnabled"))
        .and_then(Value::as_bool)
    {
        out.insert(
            "advanced".to_string(),
            json!({ "debugEnabled": debug_enabled }),
        );
    }
    if let Some(log_level) = root.and_then(|r| r.get("logLevel")).and_then(Value::as_str) {
        let adv = out
            .entry("advanced".to_string())
            .or_insert_with(|| json!({}));
        if let Some(obj) = adv.as_object_mut() {
            obj.insert("logLevel".to_string(), Value::String(log_level.to_string()));
        }
    }

    if let Some(style) = root
        .and_then(|r| r.get("citationStylePolicy"))
        .and_then(Value::as_str)
    {
        out.insert("citation".to_string(), json!({ "stylePolicy": style }));
    }
    if let Some(auto) = root
        .and_then(|r| r.get("autoApplyThreshold"))
        .and_then(Value::as_f64)
    {
        let citation = out
            .entry("citation".to_string())
            .or_insert_with(|| json!({}));
        if let Some(obj) = citation.as_object_mut() {
            obj.insert("autoApplyThreshold".to_string(), Value::from(auto));
        }
    }
    if let Some(review) = root
        .and_then(|r| r.get("reviewThreshold"))
        .and_then(Value::as_f64)
    {
        let citation = out
            .entry("citation".to_string())
            .or_insert_with(|| json!({}));
        if let Some(obj) = citation.as_object_mut() {
            obj.insert("reviewThreshold".to_string(), Value::from(review));
        }
    }
    if let Some(limit) = root
        .and_then(|r| r.get("searchLimit"))
        .and_then(Value::as_f64)
    {
        let citation = out
            .entry("citation".to_string())
            .or_insert_with(|| json!({}));
        if let Some(obj) = citation.as_object_mut() {
            obj.insert("search".to_string(), json!({ "limit": limit }));
        }
    }

    if let Some(enabled) = root
        .and_then(|r| r.get("semanticScholarEnabled"))
        .and_then(Value::as_bool)
    {
        out.insert(
            "integrations".to_string(),
            json!({ "semanticScholar": { "enabled": enabled } }),
        );
    }
    if let Some(auto_sync) = root
        .and_then(|r| r.get("zoteroAutoSyncOnApply"))
        .and_then(Value::as_bool)
    {
        let integrations = out
            .entry("integrations".to_string())
            .or_insert_with(|| json!({}));
        if let Some(obj) = integrations.as_object_mut() {
            obj.insert(
                "zotero".to_string(),
                json!({ "autoSyncOnApply": auto_sync }),
            );
        }
    }

    Value::Object(out)
}

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

fn merge_global_migration_source(raw: &Value) -> Value {
    let mut source = raw.clone();
    let legacy = legacy_global_data(raw);
    merge_value(&mut source, &legacy);
    source
}

fn sanitize_global_settings(input: &Value) -> Value {
    let theme = get_enum(
        get_in(input, &["general", "theme"]),
        &["system", "light", "dark"],
        "system",
    );
    let language = get_enum(
        get_in(input, &["general", "language"]),
        &["zh-CN", "en-US"],
        "zh-CN",
    );
    let default_editor = get_enum(
        get_in(input, &["general", "openInEditor", "defaultEditor"]),
        &["cursor", "vscode", "zed", "sublime", "system"],
        "system",
    );

    let auto = get_number_in_range(get_in(input, &["citation", "autoApplyThreshold"]), 0.0, 1.0)
        .unwrap_or(0.64);
    let review = get_number_in_range(get_in(input, &["citation", "reviewThreshold"]), 0.0, 1.0)
        .unwrap_or(0.5);
    let (auto, review) = normalize_threshold_pair(auto, review);
    let search_limit =
        get_number_in_range(get_in(input, &["citation", "search", "limit"]), 1.0, 20.0)
            .unwrap_or(8.0)
            .round() as i64;
    let llm_query_enabled = get_bool(
        get_in(input, &["citation", "search", "llmQuery", "enabled"]),
        false,
    );
    let llm_query_model = get_string_or(
        get_in(input, &["citation", "search", "llmQuery", "model"]),
        "gpt-4o-mini",
    );
    let llm_query_endpoint = get_string_or(
        get_in(input, &["citation", "search", "llmQuery", "endpoint"]),
        "https://api.openai.com/v1/chat/completions",
    );
    let llm_query_timeout_ms = get_number_in_range(
        get_in(input, &["citation", "search", "llmQuery", "timeoutMs"]),
        2000.0,
        20000.0,
    )
    .unwrap_or(6000.0)
    .round() as i64;
    let llm_query_max_queries = get_number_in_range(
        get_in(input, &["citation", "search", "llmQuery", "maxQueries"]),
        1.0,
        6.0,
    )
    .unwrap_or(3.0)
    .round() as i64;
    let query_embedding_provider = get_enum(
        get_in(input, &["citation", "search", "queryEmbedding", "provider"]),
        &["none", "local_embedding"],
        "none",
    );
    let query_embedding_timeout_ms = get_number_in_range(
        get_in(
            input,
            &["citation", "search", "queryEmbedding", "timeoutMs"],
        ),
        100.0,
        10000.0,
    )
    .unwrap_or(1200.0)
    .round() as i64;
    let query_execution_top_n = get_number_in_range(
        get_in(input, &["citation", "search", "queryExecution", "topN"]),
        1.0,
        10.0,
    )
    .unwrap_or(5.0)
    .round() as i64;
    let query_execution_mmr_lambda = get_number_in_range(
        get_in(
            input,
            &["citation", "search", "queryExecution", "mmrLambda"],
        ),
        0.0,
        1.0,
    )
    .unwrap_or(0.72);
    let query_execution_min_quality = get_number_in_range(
        get_in(
            input,
            &["citation", "search", "queryExecution", "minQuality"],
        ),
        0.0,
        1.0,
    )
    .unwrap_or(0.24);
    let query_execution_min_hit_ratio = get_number_in_range(
        get_in(
            input,
            &["citation", "search", "queryExecution", "minHitRatio"],
        ),
        0.0,
        1.0,
    )
    .unwrap_or(0.45);
    let query_execution_hit_score_threshold = get_number_in_range(
        get_in(
            input,
            &["citation", "search", "queryExecution", "hitScoreThreshold"],
        ),
        0.0,
        1.0,
    )
    .unwrap_or(0.58);

    let style_policy = get_enum(
        get_in(input, &["citation", "stylePolicy"]),
        &["auto", "cite", "citep", "autocite"],
        "auto",
    );
    let agent_runtime = get_enum(
        get_in(input, &["integrations", "agent", "runtime"]),
        &["claude_cli", "local_agent"],
        "claude_cli",
    );
    let agent_provider = get_enum(
        get_in(input, &["integrations", "agent", "provider"]),
        &["openai", "minimax", "deepseek"],
        "openai",
    );

    let agent_model = get_string_or(
        get_in(input, &["integrations", "agent", "model"]),
        "gpt-5.4",
    );
    let agent_base_url = get_string_or(
        get_in(input, &["integrations", "agent", "baseUrl"]),
        "https://api.openai.com/v1",
    );
    let agent_edit_stable_temperature = get_number_in_range(
        get_in(
            input,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                "editStable",
                "temperature",
            ],
        ),
        0.0,
        2.0,
    )
    .unwrap_or(0.2);
    let agent_edit_stable_top_p = get_number_in_range(
        get_in(
            input,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                "editStable",
                "topP",
            ],
        ),
        0.0,
        1.0,
    )
    .unwrap_or(0.9);
    let agent_edit_stable_max_tokens = get_number_in_range(
        get_in(
            input,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                "editStable",
                "maxTokens",
            ],
        ),
        256.0,
        16384.0,
    )
    .map(|v| v.round() as i64)
    .unwrap_or(8192);
    let agent_analysis_balanced_temperature = get_number_in_range(
        get_in(
            input,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                "analysisBalanced",
                "temperature",
            ],
        ),
        0.0,
        2.0,
    )
    .unwrap_or(0.4);
    let agent_analysis_balanced_top_p = get_number_in_range(
        get_in(
            input,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                "analysisBalanced",
                "topP",
            ],
        ),
        0.0,
        1.0,
    )
    .unwrap_or(0.9);
    let agent_analysis_balanced_max_tokens = get_number_in_range(
        get_in(
            input,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                "analysisBalanced",
                "maxTokens",
            ],
        ),
        256.0,
        16384.0,
    )
    .map(|v| v.round() as i64)
    .unwrap_or(6144);
    let agent_analysis_deep_temperature = get_number_in_range(
        get_in(
            input,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                "analysisDeep",
                "temperature",
            ],
        ),
        0.0,
        2.0,
    )
    .unwrap_or(0.3);
    let agent_analysis_deep_top_p = get_number_in_range(
        get_in(
            input,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                "analysisDeep",
                "topP",
            ],
        ),
        0.0,
        1.0,
    )
    .unwrap_or(0.92);
    let agent_analysis_deep_max_tokens = get_number_in_range(
        get_in(
            input,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                "analysisDeep",
                "maxTokens",
            ],
        ),
        256.0,
        16384.0,
    )
    .map(|v| v.round() as i64)
    .unwrap_or(12288);
    let agent_chat_flexible_temperature = get_number_in_range(
        get_in(
            input,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                "chatFlexible",
                "temperature",
            ],
        ),
        0.0,
        2.0,
    )
    .unwrap_or(0.7);
    let agent_chat_flexible_top_p = get_number_in_range(
        get_in(
            input,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                "chatFlexible",
                "topP",
            ],
        ),
        0.0,
        1.0,
    )
    .unwrap_or(0.95);
    let agent_chat_flexible_max_tokens = get_number_in_range(
        get_in(
            input,
            &[
                "integrations",
                "agent",
                "samplingProfiles",
                "chatFlexible",
                "maxTokens",
            ],
        ),
        256.0,
        16384.0,
    )
    .map(|v| v.round() as i64)
    .unwrap_or(4096);
    let semantic_scholar_enabled = get_bool(
        get_in(input, &["integrations", "semanticScholar", "enabled"]),
        true,
    );
    let zotero_auto_sync = get_bool(
        get_in(input, &["integrations", "zotero", "autoSyncOnApply"]),
        true,
    );
    let debug_enabled = get_bool(get_in(input, &["advanced", "debugEnabled"]), false);
    let log_level = get_enum(
        get_in(input, &["advanced", "logLevel"]),
        &["info", "debug", "warn", "error"],
        "info",
    );

    json!({
      "general": {
        "theme": theme,
        "language": language,
        "openInEditor": {
          "defaultEditor": default_editor
        }
      },
      "citation": {
        "stylePolicy": style_policy,
        "autoApplyThreshold": auto,
        "reviewThreshold": review,
        "search": {
          "limit": search_limit,
          "llmQuery": {
            "enabled": llm_query_enabled,
            "model": llm_query_model,
            "endpoint": llm_query_endpoint,
            "timeoutMs": llm_query_timeout_ms,
            "maxQueries": llm_query_max_queries
          },
          "queryEmbedding": {
            "provider": query_embedding_provider,
            "timeoutMs": query_embedding_timeout_ms
          },
          "queryExecution": {
            "topN": query_execution_top_n,
            "mmrLambda": query_execution_mmr_lambda,
            "minQuality": query_execution_min_quality,
            "minHitRatio": query_execution_min_hit_ratio,
            "hitScoreThreshold": query_execution_hit_score_threshold
          }
        }
      },
      "integrations": {
        "agent": {
          "runtime": agent_runtime,
          "provider": agent_provider,
          "model": agent_model,
          "baseUrl": agent_base_url,
          "samplingProfiles": {
            "editStable": {
              "temperature": agent_edit_stable_temperature,
              "topP": agent_edit_stable_top_p,
              "maxTokens": agent_edit_stable_max_tokens
            },
            "analysisBalanced": {
              "temperature": agent_analysis_balanced_temperature,
              "topP": agent_analysis_balanced_top_p,
              "maxTokens": agent_analysis_balanced_max_tokens
            },
            "analysisDeep": {
              "temperature": agent_analysis_deep_temperature,
              "topP": agent_analysis_deep_top_p,
              "maxTokens": agent_analysis_deep_max_tokens
            },
            "chatFlexible": {
              "temperature": agent_chat_flexible_temperature,
              "topP": agent_chat_flexible_top_p,
              "maxTokens": agent_chat_flexible_max_tokens
            }
          }
        },
        "semanticScholar": {
          "enabled": semantic_scholar_enabled
        },
        "zotero": {
          "autoSyncOnApply": zotero_auto_sync
        }
      },
      "advanced": {
        "debugEnabled": debug_enabled,
        "logLevel": log_level
      }
    })
}

fn sanitize_project_settings(input: &Value) -> Value {
    let auto =
        get_number_in_range_strict(get_in(input, &["citation", "autoApplyThreshold"]), 0.0, 1.0);
    let review =
        get_number_in_range_strict(get_in(input, &["citation", "reviewThreshold"]), 0.0, 1.0);
    let limit =
        get_number_in_range_strict(get_in(input, &["citation", "search", "limit"]), 1.0, 20.0)
            .map(|v| v.round() as i64);

    let mut citation = Map::<String, Value>::new();
    if let Some(a) = auto {
        citation.insert("autoApplyThreshold".to_string(), Value::from(a));
    }
    if let Some(r) = review {
        citation.insert("reviewThreshold".to_string(), Value::from(r));
    }
    if let Some(l) = limit {
        citation.insert("search".to_string(), json!({ "limit": l }));
    }

    if let (Some(a), Some(r)) = (
        citation.get("autoApplyThreshold").and_then(Value::as_f64),
        citation.get("reviewThreshold").and_then(Value::as_f64),
    ) {
        citation.insert("reviewThreshold".to_string(), Value::from(r.min(a)));
    }

    if citation.is_empty() {
        json!({})
    } else {
        json!({ "citation": Value::Object(citation) })
    }
}

fn sanitize_secret_settings(input: &Value) -> Value {
    let agent_api_key = get_string_or_null(get_in(input, &["integrations", "agent", "apiKey"]));
    let api_key = get_string_or_null(get_in(
        input,
        &["integrations", "semanticScholar", "apiKey"],
    ));
    let llm_api_key = get_string_or_null(get_in(input, &["integrations", "llmQuery", "apiKey"]));
    json!({
      "integrations": {
        "agent": {
          "apiKey": agent_api_key
        },
        "semanticScholar": {
          "apiKey": api_key
        },
        "llmQuery": {
          "apiKey": llm_api_key
        }
      }
    })
}

fn migrate_global_envelope(raw: &Value) -> SettingsEnvelope {
    if let Some(obj) = as_object(raw) {
        if obj.get("version").and_then(Value::as_u64).is_some() {
            if let Some(data) = obj.get("data") {
                return SettingsEnvelope {
                    version: SETTINGS_SCHEMA_VERSION,
                    data: sanitize_global_settings(data),
                };
            }
        }
    }

    SettingsEnvelope {
        version: SETTINGS_SCHEMA_VERSION,
        data: sanitize_global_settings(&merge_global_migration_source(raw)),
    }
}

fn migrate_project_envelope(raw: &Value) -> SettingsEnvelope {
    if let Some(obj) = as_object(raw) {
        if obj.get("version").and_then(Value::as_u64).is_some() {
            if let Some(data) = obj.get("data") {
                return SettingsEnvelope {
                    version: SETTINGS_SCHEMA_VERSION,
                    data: sanitize_project_settings(data),
                };
            }
        }
    }
    SettingsEnvelope {
        version: SETTINGS_SCHEMA_VERSION,
        data: sanitize_project_settings(raw),
    }
}

fn migrate_secret_envelope(raw: &Value) -> SettingsEnvelope {
    if let Some(obj) = as_object(raw) {
        if obj.get("version").and_then(Value::as_u64).is_some() {
            if let Some(data) = obj.get("data") {
                return SettingsEnvelope {
                    version: SETTINGS_SCHEMA_VERSION,
                    data: sanitize_secret_settings(data),
                };
            }
        }
    }
    SettingsEnvelope {
        version: SETTINGS_SCHEMA_VERSION,
        data: sanitize_secret_settings(raw),
    }
}

fn resolve_effective_settings(global_input: &Value, project_input: &Value) -> Value {
    let global = sanitize_global_settings(global_input);
    let project = sanitize_project_settings(project_input);

    let auto_apply = get_in(&project, &["citation", "autoApplyThreshold"])
        .and_then(Value::as_f64)
        .unwrap_or_else(|| {
            get_in(&global, &["citation", "autoApplyThreshold"])
                .and_then(Value::as_f64)
                .unwrap_or(0.64)
        });
    let review = get_in(&project, &["citation", "reviewThreshold"])
        .and_then(Value::as_f64)
        .unwrap_or_else(|| {
            get_in(&global, &["citation", "reviewThreshold"])
                .and_then(Value::as_f64)
                .unwrap_or(0.5)
        })
        .min(auto_apply);
    let search_limit = get_in(&project, &["citation", "search", "limit"])
        .and_then(Value::as_i64)
        .unwrap_or_else(|| {
            get_in(&global, &["citation", "search", "limit"])
                .and_then(Value::as_i64)
                .unwrap_or(8)
        });
    let llm_query = get_in(&global, &["citation", "search", "llmQuery"])
        .cloned()
        .unwrap_or_else(|| {
            json!({
              "enabled": false,
              "model": "gpt-4o-mini",
              "endpoint": "https://api.openai.com/v1/chat/completions",
              "timeoutMs": 6000,
              "maxQueries": 3
            })
        });
    let query_embedding = get_in(&global, &["citation", "search", "queryEmbedding"])
        .cloned()
        .unwrap_or_else(|| {
            json!({
              "provider": "none",
              "timeoutMs": 1200
            })
        });
    let query_execution = get_in(&global, &["citation", "search", "queryExecution"])
        .cloned()
        .unwrap_or_else(|| {
            json!({
              "topN": 5,
              "mmrLambda": 0.72,
              "minQuality": 0.24,
              "minHitRatio": 0.45,
              "hitScoreThreshold": 0.58
            })
        });

    json!({
      "general": get_in(&global, &["general"]).cloned().unwrap_or_else(|| json!({})),
      "citation": {
        "stylePolicy": get_in(&global, &["citation", "stylePolicy"]).cloned().unwrap_or_else(|| json!("auto")),
        "autoApplyThreshold": auto_apply,
        "reviewThreshold": review,
        "search": {
          "limit": search_limit,
          "llmQuery": llm_query,
          "queryEmbedding": query_embedding,
          "queryExecution": query_execution
        }
      },
      "integrations": get_in(&global, &["integrations"]).cloned().unwrap_or_else(|| json!({
        "agent": {
          "runtime": "claude_cli",
          "provider": "openai",
          "model": "gpt-5.4",
          "baseUrl": "https://api.openai.com/v1",
          "samplingProfiles": {
            "editStable": {
              "temperature": 0.2,
              "topP": 0.9,
              "maxTokens": 8192
            },
            "analysisBalanced": {
              "temperature": 0.4,
              "topP": 0.9,
              "maxTokens": 6144
            },
            "analysisDeep": {
              "temperature": 0.3,
              "topP": 0.92,
              "maxTokens": 12288
            },
            "chatFlexible": {
              "temperature": 0.7,
              "topP": 0.95,
              "maxTokens": 4096
            }
          }
        }
      })),
      "advanced": get_in(&global, &["advanced"]).cloned().unwrap_or_else(|| json!({}))
    })
}

fn to_secrets_meta(secret_input: &Value) -> Value {
    let secret = sanitize_secret_settings(secret_input);
    let agent_configured = resolve_secret_value(
        &secret,
        &["integrations", "agent", "apiKey"],
        KEYCHAIN_SERVICE_AGENT_OPENAI,
    )
    .is_some();
    let configured = resolve_secret_value(
        &secret,
        &["integrations", "semanticScholar", "apiKey"],
        KEYCHAIN_SERVICE_SEMANTIC_SCHOLAR,
    )
    .is_some();
    let llm_configured = resolve_secret_value(
        &secret,
        &["integrations", "llmQuery", "apiKey"],
        KEYCHAIN_SERVICE_LLM_QUERY,
    )
    .is_some();
    json!({
      "integrations": {
        "agent": {
          "apiKeyConfigured": agent_configured
        },
        "semanticScholar": {
          "apiKeyConfigured": configured
        },
        "llmQuery": {
          "apiKeyConfigured": llm_configured
        }
      }
    })
}

fn is_canonical_envelope_data(raw: &Value, migrated: &SettingsEnvelope) -> bool {
    let obj = match as_object(raw) {
        Some(v) => v,
        None => return false,
    };
    let version_ok = obj
        .get("version")
        .and_then(Value::as_u64)
        .map(|v| v == SETTINGS_SCHEMA_VERSION as u64)
        .unwrap_or(false);
    if !version_ok {
        return false;
    }
    match obj.get("data") {
        Some(data) => data == &migrated.data,
        None => false,
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

fn push_error(errors: &mut Vec<SettingsFieldError>, path: &str, message: &str) {
    errors.push(SettingsFieldError {
        path: path.to_string(),
        message: message.to_string(),
    });
}

fn validate_enum(
    value: &Value,
    path: &str,
    allowed: &[&str],
    errors: &mut Vec<SettingsFieldError>,
) {
    match value.as_str() {
        Some(v) if allowed.contains(&v) => {}
        Some(_) => push_error(errors, path, "invalid enum value"),
        None => push_error(errors, path, "must be a string"),
    }
}

fn validate_number_range(
    value: &Value,
    path: &str,
    min: f64,
    max: f64,
    errors: &mut Vec<SettingsFieldError>,
) {
    match value.as_f64() {
        Some(v) if v.is_finite() && v >= min && v <= max => {}
        Some(_) => push_error(errors, path, "number out of allowed range"),
        None => push_error(errors, path, "must be a number"),
    }
}

fn validate_bool(value: &Value, path: &str, errors: &mut Vec<SettingsFieldError>) {
    if !value.is_boolean() {
        push_error(errors, path, "must be a boolean");
    }
}

fn validate_non_empty_string(value: &Value, path: &str, errors: &mut Vec<SettingsFieldError>) {
    match value.as_str() {
        Some(v) if !v.trim().is_empty() => {}
        Some(_) => push_error(errors, path, "must be a non-empty string"),
        None => push_error(errors, path, "must be a string"),
    }
}

fn validate_object<'a>(
    value: &'a Value,
    path: &str,
    errors: &mut Vec<SettingsFieldError>,
) -> Option<&'a Map<String, Value>> {
    match value {
        Value::Object(obj) => Some(obj),
        _ => {
            push_error(errors, path, "must be an object");
            None
        }
    }
}

fn validate_global_patch(patch: &Value) -> Vec<SettingsFieldError> {
    let mut errors = Vec::<SettingsFieldError>::new();
    let Some(root) = validate_object(patch, "$", &mut errors) else {
        return errors;
    };

    for (k, v) in root {
        match k.as_str() {
            "general" => {
                if let Some(general) = validate_object(v, "general", &mut errors) {
                    for (gk, gv) in general {
                        match gk.as_str() {
                            "theme" => validate_enum(
                                gv,
                                "general.theme",
                                &["system", "light", "dark"],
                                &mut errors,
                            ),
                            "language" => validate_enum(
                                gv,
                                "general.language",
                                &["zh-CN", "en-US"],
                                &mut errors,
                            ),
                            "openInEditor" => {
                                if let Some(oe) =
                                    validate_object(gv, "general.openInEditor", &mut errors)
                                {
                                    for (oek, oev) in oe {
                                        match oek.as_str() {
                                            "defaultEditor" => validate_enum(
                                                oev,
                                                "general.openInEditor.defaultEditor",
                                                &["cursor", "vscode", "zed", "sublime", "system"],
                                                &mut errors,
                                            ),
                                            _ => push_error(
                                                &mut errors,
                                                "general.openInEditor",
                                                "unknown field",
                                            ),
                                        }
                                    }
                                }
                            }
                            _ => push_error(&mut errors, "general", "unknown field"),
                        }
                    }
                }
            }
            "citation" => {
                if let Some(citation) = validate_object(v, "citation", &mut errors) {
                    for (ck, cv) in citation {
                        match ck.as_str() {
                            "stylePolicy" => validate_enum(
                                cv,
                                "citation.stylePolicy",
                                &["auto", "cite", "citep", "autocite"],
                                &mut errors,
                            ),
                            "autoApplyThreshold" => validate_number_range(
                                cv,
                                "citation.autoApplyThreshold",
                                0.0,
                                1.0,
                                &mut errors,
                            ),
                            "reviewThreshold" => validate_number_range(
                                cv,
                                "citation.reviewThreshold",
                                0.0,
                                1.0,
                                &mut errors,
                            ),
                            "search" => {
                                if let Some(search) =
                                    validate_object(cv, "citation.search", &mut errors)
                                {
                                    for (sk, sv) in search {
                                        match sk.as_str() {
                                            "limit" => validate_number_range(
                                                sv,
                                                "citation.search.limit",
                                                1.0,
                                                20.0,
                                                &mut errors,
                                            ),
                                            "llmQuery" => {
                                                if let Some(llm) = validate_object(
                                                    sv,
                                                    "citation.search.llmQuery",
                                                    &mut errors,
                                                ) {
                                                    for (lk, lv) in llm {
                                                        match lk.as_str() {
                                                            "enabled" => validate_bool(
                                                                lv,
                                                                "citation.search.llmQuery.enabled",
                                                                &mut errors,
                                                            ),
                                                            "model" => {
                                                                if !lv.is_string() {
                                                                    push_error(
                                                                        &mut errors,
                                                                        "citation.search.llmQuery.model",
                                                                        "must be a string",
                                                                    );
                                                                }
                                                            }
                                                            "endpoint" => {
                                                                if !lv.is_string() {
                                                                    push_error(
                                                                        &mut errors,
                                                                        "citation.search.llmQuery.endpoint",
                                                                        "must be a string",
                                                                    );
                                                                }
                                                            }
                                                            "timeoutMs" => validate_number_range(
                                                                lv,
                                                                "citation.search.llmQuery.timeoutMs",
                                                                2000.0,
                                                                20000.0,
                                                                &mut errors,
                                                            ),
                                                            "maxQueries" => validate_number_range(
                                                                lv,
                                                                "citation.search.llmQuery.maxQueries",
                                                                1.0,
                                                                6.0,
                                                                &mut errors,
                                                            ),
                                                            _ => push_error(
                                                                &mut errors,
                                                                "citation.search.llmQuery",
                                                                "unknown field",
                                                            ),
                                                        }
                                                    }
                                                }
                                            }
                                            "queryEmbedding" => {
                                                if let Some(embedding) = validate_object(
                                                    sv,
                                                    "citation.search.queryEmbedding",
                                                    &mut errors,
                                                ) {
                                                    for (ek, ev) in embedding {
                                                        match ek.as_str() {
                                                            "provider" => validate_enum(
                                                                ev,
                                                                "citation.search.queryEmbedding.provider",
                                                                &["none", "local_embedding"],
                                                                &mut errors,
                                                            ),
                                                            "timeoutMs" => validate_number_range(
                                                                ev,
                                                                "citation.search.queryEmbedding.timeoutMs",
                                                                100.0,
                                                                10000.0,
                                                                &mut errors,
                                                            ),
                                                            _ => push_error(
                                                                &mut errors,
                                                                "citation.search.queryEmbedding",
                                                                "unknown field",
                                                            ),
                                                        }
                                                    }
                                                }
                                            }
                                            "queryExecution" => {
                                                if let Some(exec_cfg) = validate_object(
                                                    sv,
                                                    "citation.search.queryExecution",
                                                    &mut errors,
                                                ) {
                                                    for (ek, ev) in exec_cfg {
                                                        match ek.as_str() {
                                                            "topN" => validate_number_range(
                                                                ev,
                                                                "citation.search.queryExecution.topN",
                                                                1.0,
                                                                10.0,
                                                                &mut errors,
                                                            ),
                                                            "mmrLambda" => validate_number_range(
                                                                ev,
                                                                "citation.search.queryExecution.mmrLambda",
                                                                0.0,
                                                                1.0,
                                                                &mut errors,
                                                            ),
                                                            "minQuality" => validate_number_range(
                                                                ev,
                                                                "citation.search.queryExecution.minQuality",
                                                                0.0,
                                                                1.0,
                                                                &mut errors,
                                                            ),
                                                            "minHitRatio" => validate_number_range(
                                                                ev,
                                                                "citation.search.queryExecution.minHitRatio",
                                                                0.0,
                                                                1.0,
                                                                &mut errors,
                                                            ),
                                                            "hitScoreThreshold" => {
                                                                validate_number_range(
                                                                    ev,
                                                                    "citation.search.queryExecution.hitScoreThreshold",
                                                                    0.0,
                                                                    1.0,
                                                                    &mut errors,
                                                                )
                                                            }
                                                            _ => push_error(
                                                                &mut errors,
                                                                "citation.search.queryExecution",
                                                                "unknown field",
                                                            ),
                                                        }
                                                    }
                                                }
                                            }
                                            _ => push_error(
                                                &mut errors,
                                                "citation.search",
                                                "unknown field",
                                            ),
                                        }
                                    }
                                }
                            }
                            _ => push_error(&mut errors, "citation", "unknown field"),
                        }
                    }
                }
            }
            "integrations" => {
                if let Some(integrations) = validate_object(v, "integrations", &mut errors) {
                    for (ik, iv) in integrations {
                        match ik.as_str() {
                            "agent" => {
                                if let Some(agent) =
                                    validate_object(iv, "integrations.agent", &mut errors)
                                {
                                    for (ak, av) in agent {
                                        match ak.as_str() {
                                            "runtime" => validate_enum(
                                                av,
                                                "integrations.agent.runtime",
                                                &["claude_cli", "local_agent"],
                                                &mut errors,
                                            ),
                                            "provider" => validate_enum(
                                                av,
                                                "integrations.agent.provider",
                                                &["openai", "minimax", "deepseek"],
                                                &mut errors,
                                            ),
                                            "model" => validate_non_empty_string(
                                                av,
                                                "integrations.agent.model",
                                                &mut errors,
                                            ),
                                            "baseUrl" => validate_non_empty_string(
                                                av,
                                                "integrations.agent.baseUrl",
                                                &mut errors,
                                            ),
                                            "samplingProfiles" => {
                                                if let Some(profiles) = validate_object(
                                                    av,
                                                    "integrations.agent.samplingProfiles",
                                                    &mut errors,
                                                ) {
                                                    for (pk, pv) in profiles {
                                                        let profile_path = format!(
                                                            "integrations.agent.samplingProfiles.{}",
                                                            pk
                                                        );
                                                        if let Some(profile) = validate_object(
                                                            pv,
                                                            &profile_path,
                                                            &mut errors,
                                                        ) {
                                                            for (sk, sv) in profile {
                                                                match sk.as_str() {
                                                                    "temperature" => {
                                                                        validate_number_range(
                                                                            sv,
                                                                            &format!(
                                                                                "{}.temperature",
                                                                                profile_path
                                                                            ),
                                                                            0.0,
                                                                            2.0,
                                                                            &mut errors,
                                                                        )
                                                                    }
                                                                    "topP" => {
                                                                        validate_number_range(
                                                                            sv,
                                                                            &format!(
                                                                                "{}.topP",
                                                                                profile_path
                                                                            ),
                                                                            0.0,
                                                                            1.0,
                                                                            &mut errors,
                                                                        )
                                                                    }
                                                                    "maxTokens" => {
                                                                        validate_number_range(
                                                                            sv,
                                                                            &format!(
                                                                                "{}.maxTokens",
                                                                                profile_path
                                                                            ),
                                                                            256.0,
                                                                            16384.0,
                                                                            &mut errors,
                                                                        )
                                                                    }
                                                                    _ => push_error(
                                                                        &mut errors,
                                                                        &profile_path,
                                                                        "unknown field",
                                                                    ),
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            _ => push_error(
                                                &mut errors,
                                                "integrations.agent",
                                                "unknown field",
                                            ),
                                        }
                                    }
                                }
                            }
                            "semanticScholar" => {
                                if let Some(ss) =
                                    validate_object(iv, "integrations.semanticScholar", &mut errors)
                                {
                                    for (ssk, ssv) in ss {
                                        match ssk.as_str() {
                                            "enabled" => validate_bool(
                                                ssv,
                                                "integrations.semanticScholar.enabled",
                                                &mut errors,
                                            ),
                                            _ => push_error(
                                                &mut errors,
                                                "integrations.semanticScholar",
                                                "unknown field",
                                            ),
                                        }
                                    }
                                }
                            }
                            "zotero" => {
                                if let Some(zo) =
                                    validate_object(iv, "integrations.zotero", &mut errors)
                                {
                                    for (zk, zv) in zo {
                                        match zk.as_str() {
                                            "autoSyncOnApply" => validate_bool(
                                                zv,
                                                "integrations.zotero.autoSyncOnApply",
                                                &mut errors,
                                            ),
                                            _ => push_error(
                                                &mut errors,
                                                "integrations.zotero",
                                                "unknown field",
                                            ),
                                        }
                                    }
                                }
                            }
                            _ => push_error(&mut errors, "integrations", "unknown field"),
                        }
                    }
                }
            }
            "advanced" => {
                if let Some(advanced) = validate_object(v, "advanced", &mut errors) {
                    for (ak, av) in advanced {
                        match ak.as_str() {
                            "debugEnabled" => {
                                validate_bool(av, "advanced.debugEnabled", &mut errors)
                            }
                            "logLevel" => validate_enum(
                                av,
                                "advanced.logLevel",
                                &["info", "debug", "warn", "error"],
                                &mut errors,
                            ),
                            _ => push_error(&mut errors, "advanced", "unknown field"),
                        }
                    }
                }
            }
            _ => push_error(&mut errors, "$", "unknown field"),
        }
    }

    errors
}

fn validate_project_patch(patch: &Value) -> Vec<SettingsFieldError> {
    let mut errors = Vec::<SettingsFieldError>::new();
    let Some(root) = validate_object(patch, "$", &mut errors) else {
        return errors;
    };

    for (k, v) in root {
        match k.as_str() {
            "citation" => {
                if let Some(citation) = validate_object(v, "citation", &mut errors) {
                    for (ck, cv) in citation {
                        match ck.as_str() {
                            "autoApplyThreshold" => validate_number_range(
                                cv,
                                "citation.autoApplyThreshold",
                                0.0,
                                1.0,
                                &mut errors,
                            ),
                            "reviewThreshold" => validate_number_range(
                                cv,
                                "citation.reviewThreshold",
                                0.0,
                                1.0,
                                &mut errors,
                            ),
                            "search" => {
                                if let Some(search) =
                                    validate_object(cv, "citation.search", &mut errors)
                                {
                                    for (sk, sv) in search {
                                        match sk.as_str() {
                                            "limit" => validate_number_range(
                                                sv,
                                                "citation.search.limit",
                                                1.0,
                                                20.0,
                                                &mut errors,
                                            ),
                                            _ => push_error(
                                                &mut errors,
                                                "citation.search",
                                                "unknown field",
                                            ),
                                        }
                                    }
                                }
                            }
                            _ => push_error(&mut errors, "citation", "unknown field"),
                        }
                    }
                }
            }
            _ => push_error(&mut errors, "$", "unknown field"),
        }
    }
    errors
}

fn validate_secret_patch(patch: &Value) -> Vec<SettingsFieldError> {
    let mut errors = Vec::<SettingsFieldError>::new();
    let Some(root) = validate_object(patch, "$", &mut errors) else {
        return errors;
    };

    for (k, v) in root {
        match k.as_str() {
            "integrations" => {
                if let Some(integrations) = validate_object(v, "integrations", &mut errors) {
                    for (ik, iv) in integrations {
                        match ik.as_str() {
                            "agent" => {
                                if let Some(agent) =
                                    validate_object(iv, "integrations.agent", &mut errors)
                                {
                                    for (ak, av) in agent {
                                        match ak.as_str() {
                                            "apiKey" => {
                                                if !(av.is_null() || av.is_string()) {
                                                    push_error(
                                                        &mut errors,
                                                        "integrations.agent.apiKey",
                                                        "must be string or null",
                                                    );
                                                }
                                            }
                                            _ => push_error(
                                                &mut errors,
                                                "integrations.agent",
                                                "unknown field",
                                            ),
                                        }
                                    }
                                }
                            }
                            "semanticScholar" => {
                                if let Some(ss) =
                                    validate_object(iv, "integrations.semanticScholar", &mut errors)
                                {
                                    for (ssk, ssv) in ss {
                                        match ssk.as_str() {
                                            "apiKey" => {
                                                if !(ssv.is_null() || ssv.is_string()) {
                                                    push_error(
                                                        &mut errors,
                                                        "integrations.semanticScholar.apiKey",
                                                        "must be string or null",
                                                    );
                                                }
                                            }
                                            _ => push_error(
                                                &mut errors,
                                                "integrations.semanticScholar",
                                                "unknown field",
                                            ),
                                        }
                                    }
                                }
                            }
                            "llmQuery" => {
                                if let Some(lq) =
                                    validate_object(iv, "integrations.llmQuery", &mut errors)
                                {
                                    for (lqk, lqv) in lq {
                                        match lqk.as_str() {
                                            "apiKey" => {
                                                if !(lqv.is_null() || lqv.is_string()) {
                                                    push_error(
                                                        &mut errors,
                                                        "integrations.llmQuery.apiKey",
                                                        "must be string or null",
                                                    );
                                                }
                                            }
                                            _ => push_error(
                                                &mut errors,
                                                "integrations.llmQuery",
                                                "unknown field",
                                            ),
                                        }
                                    }
                                }
                            }
                            _ => push_error(&mut errors, "integrations", "unknown field"),
                        }
                    }
                }
            }
            _ => push_error(&mut errors, "$", "unknown field"),
        }
    }
    errors
}

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

#[derive(Debug, Clone)]
pub(crate) struct AgentSamplingConfig {
    pub temperature: f64,
    pub top_p: f64,
    pub max_tokens: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentSamplingProfilesConfig {
    pub edit_stable: AgentSamplingConfig,
    pub analysis_balanced: AgentSamplingConfig,
    pub analysis_deep: AgentSamplingConfig,
    pub chat_flexible: AgentSamplingConfig,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentRuntimeConfig {
    pub runtime: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub sampling_profiles: AgentSamplingProfilesConfig,
}

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

fn read_optional_secret(secret: &Value, path: &[&str]) -> Option<String> {
    get_in(secret, path)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

fn set_secret_api_key(secret: &mut Value, provider: &str, value: Option<String>) {
    let Some(root) = secret.as_object_mut() else {
        return;
    };
    let Some(integrations) = root.get_mut("integrations").and_then(Value::as_object_mut) else {
        return;
    };
    let Some(provider_obj) = integrations
        .get_mut(provider)
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    provider_obj.insert(
        "apiKey".to_string(),
        value.map(Value::String).unwrap_or(Value::Null),
    );
}

#[cfg(target_os = "macos")]
fn keychain_store_secret(service: &str, value: &str) -> Result<bool, String> {
    let output = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-a",
            KEYCHAIN_ACCOUNT,
            "-s",
            service,
            "-w",
            value,
            "-U",
        ])
        .output()
        .map_err(|e| format!("failed to call security add-generic-password: {}", e))?;
    if output.status.success() {
        return Ok(true);
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

#[cfg(not(target_os = "macos"))]
fn keychain_store_secret(_service: &str, _value: &str) -> Result<bool, String> {
    Ok(false)
}

#[cfg(target_os = "macos")]
fn keychain_read_secret(service: &str) -> Result<Option<String>, String> {
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-a",
            KEYCHAIN_ACCOUNT,
            "-s",
            service,
            "-w",
        ])
        .output()
        .map_err(|e| format!("failed to call security find-generic-password: {}", e))?;
    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if value.is_empty() {
            return Ok(None);
        }
        return Ok(Some(value));
    }
    let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();
    if stderr_text.contains("could not be found") {
        return Ok(None);
    }
    Err(stderr_text.trim().to_string())
}

#[cfg(not(target_os = "macos"))]
fn keychain_read_secret(_service: &str) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(target_os = "macos")]
fn keychain_delete_secret(service: &str) -> Result<bool, String> {
    let output = std::process::Command::new("security")
        .args([
            "delete-generic-password",
            "-a",
            KEYCHAIN_ACCOUNT,
            "-s",
            service,
        ])
        .output()
        .map_err(|e| format!("failed to call security delete-generic-password: {}", e))?;
    if output.status.success() {
        return Ok(true);
    }
    let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();
    if stderr_text.contains("could not be found") {
        return Ok(true);
    }
    Err(stderr_text.trim().to_string())
}

#[cfg(not(target_os = "macos"))]
fn keychain_delete_secret(_service: &str) -> Result<bool, String> {
    Ok(false)
}

fn resolve_secret_value(secret: &Value, path: &[&str], service: &str) -> Option<String> {
    if let Some(value) = read_optional_secret(secret, path) {
        return Some(value);
    }
    keychain_read_secret(service).ok().flatten()
}

fn migrate_secret_values_to_keychain(secret: &mut Value, warnings: &mut Vec<String>) -> bool {
    let mut changed = false;

    let agent_in_file = read_optional_secret(secret, &["integrations", "agent", "apiKey"]);
    if let Some(value) = agent_in_file {
        match keychain_store_secret(KEYCHAIN_SERVICE_AGENT_OPENAI, &value) {
            Ok(true) => {
                set_secret_api_key(secret, "agent", None);
                warnings
                    .push("secret: migrated Agent OpenAI API key to system keychain".to_string());
                changed = true;
            }
            Ok(false) => {}
            Err(err) => warnings.push(format!(
                "secret: keychain unavailable for Agent OpenAI key, keeping file fallback ({})",
                err
            )),
        }
    }

    let semantic_in_file =
        read_optional_secret(secret, &["integrations", "semanticScholar", "apiKey"]);
    if let Some(value) = semantic_in_file {
        match keychain_store_secret(KEYCHAIN_SERVICE_SEMANTIC_SCHOLAR, &value) {
            Ok(true) => {
                set_secret_api_key(secret, "semanticScholar", None);
                warnings.push(
                    "secret: migrated Semantic Scholar API key to system keychain".to_string(),
                );
                changed = true;
            }
            Ok(false) => {}
            Err(err) => warnings.push(format!(
                "secret: keychain unavailable for Semantic Scholar key, keeping file fallback ({})",
                err
            )),
        }
    }

    let llm_in_file = read_optional_secret(secret, &["integrations", "llmQuery", "apiKey"]);
    if let Some(value) = llm_in_file {
        match keychain_store_secret(KEYCHAIN_SERVICE_LLM_QUERY, &value) {
            Ok(true) => {
                set_secret_api_key(secret, "llmQuery", None);
                warnings.push("secret: migrated LLM Query API key to system keychain".to_string());
                changed = true;
            }
            Ok(false) => {}
            Err(err) => warnings.push(format!(
                "secret: keychain unavailable for LLM Query key, keeping file fallback ({})",
                err
            )),
        }
    }

    changed
}

fn persist_secret_values_to_keychain_or_fallback(secret: &mut Value, warnings: &mut Vec<String>) {
    let agent = read_optional_secret(secret, &["integrations", "agent", "apiKey"]);
    match agent {
        Some(value) => match keychain_store_secret(KEYCHAIN_SERVICE_AGENT_OPENAI, &value) {
            Ok(true) => set_secret_api_key(secret, "agent", None),
            Ok(false) => warnings.push(
                "secret: keychain unavailable, Agent OpenAI key kept in fallback file".to_string(),
            ),
            Err(err) => warnings.push(format!(
                "secret: failed to write Agent OpenAI key to keychain, using file fallback ({})",
                err
            )),
        },
        None => {
            if let Err(err) = keychain_delete_secret(KEYCHAIN_SERVICE_AGENT_OPENAI) {
                warnings.push(format!(
                    "secret: failed to clear Agent OpenAI key from keychain ({})",
                    err
                ));
            }
        }
    }

    let semantic = read_optional_secret(secret, &["integrations", "semanticScholar", "apiKey"]);
    match semantic {
        Some(value) => match keychain_store_secret(KEYCHAIN_SERVICE_SEMANTIC_SCHOLAR, &value) {
            Ok(true) => set_secret_api_key(secret, "semanticScholar", None),
            Ok(false) => warnings.push(
                "secret: keychain unavailable, Semantic Scholar key kept in fallback file"
                    .to_string(),
            ),
            Err(err) => warnings.push(format!(
                "secret: failed to write Semantic Scholar key to keychain, using file fallback ({})",
                err
            )),
        },
        None => {
            if let Err(err) = keychain_delete_secret(KEYCHAIN_SERVICE_SEMANTIC_SCHOLAR) {
                warnings.push(format!(
                    "secret: failed to clear Semantic Scholar key from keychain ({})",
                    err
                ));
            }
        }
    }

    let llm = read_optional_secret(secret, &["integrations", "llmQuery", "apiKey"]);
    match llm {
        Some(value) => match keychain_store_secret(KEYCHAIN_SERVICE_LLM_QUERY, &value) {
            Ok(true) => set_secret_api_key(secret, "llmQuery", None),
            Ok(false) => warnings.push(
                "secret: keychain unavailable, LLM Query key kept in fallback file".to_string(),
            ),
            Err(err) => warnings.push(format!(
                "secret: failed to write LLM Query key to keychain, using file fallback ({})",
                err
            )),
        },
        None => {
            if let Err(err) = keychain_delete_secret(KEYCHAIN_SERVICE_LLM_QUERY) {
                warnings.push(format!(
                    "secret: failed to clear LLM Query key from keychain ({})",
                    err
                ));
            }
        }
    }
}

fn classify_http_status(status: u16) -> (bool, bool, String, String) {
    if (200..300).contains(&status) {
        return (
            true,
            true,
            "unknown".to_string(),
            format!("Connected (HTTP {}).", status),
        );
    }

    if matches!(status, 400 | 401 | 403 | 404 | 405 | 429) {
        let message = match status {
            400 => "Endpoint reachable, request rejected (HTTP 400).",
            401 => "Endpoint reachable, authorization failed (HTTP 401).",
            403 => "Endpoint reachable, access denied (HTTP 403).",
            404 => "Endpoint reachable, route not found (HTTP 404).",
            405 => "Endpoint reachable, method not allowed (HTTP 405).",
            429 => "Endpoint reachable, rate limited (HTTP 429).",
            _ => "Endpoint reachable.",
        };
        return (true, true, "unknown".to_string(), message.to_string());
    }

    (
        false,
        true,
        "unknown".to_string(),
        format!("Endpoint reachable but unhealthy (HTTP {}).", status),
    )
}

fn classify_runtime_probe_status(status: u16, capability: &str) -> (bool, bool, String, String) {
    let capability_label = match capability {
        "responses" => "Responses API",
        "chat_completions" => "Chat Completions API",
        _ => "Runtime API",
    };

    if (200..300).contains(&status) {
        return (
            true,
            true,
            "supported".to_string(),
            format!(
                "{} accepted the request (HTTP {}).",
                capability_label, status
            ),
        );
    }

    if matches!(status, 400 | 401 | 403 | 405 | 422 | 429) {
        let message = match status {
            400 => format!(
                "{} is present, but this probe payload was rejected (HTTP 400).",
                capability_label
            ),
            401 => format!(
                "{} is present, but authorization failed (HTTP 401).",
                capability_label
            ),
            403 => format!(
                "{} is present, but access was denied (HTTP 403).",
                capability_label
            ),
            405 => format!(
                "{} route exists, but the HTTP method was rejected (HTTP 405).",
                capability_label
            ),
            422 => format!(
                "{} is present, but the request body was semantically invalid (HTTP 422).",
                capability_label
            ),
            429 => format!(
                "{} is present, but the provider rate limited the probe (HTTP 429).",
                capability_label
            ),
            _ => format!("{} appears to be supported.", capability_label),
        };
        return (false, true, "supported".to_string(), message);
    }

    if status == 404 {
        return (
            false,
            true,
            "unsupported".to_string(),
            format!("{} route was not found (HTTP 404).", capability_label),
        );
    }

    (
        false,
        true,
        "unknown".to_string(),
        format!(
            "{} probe reached the server, but compatibility is unclear (HTTP {}).",
            capability_label, status
        ),
    )
}

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

impl From<SettingsEnvelope> for LoadedScope {
    fn from(envelope: SettingsEnvelope) -> Self {
        Self {
            envelope,
            warnings: Vec::new(),
            needs_write: false,
        }
    }
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
