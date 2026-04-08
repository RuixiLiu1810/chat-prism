use serde_json::{json, Map, Value};

use super::keychain::resolve_secret_value;
use super::types::SettingsEnvelope;
use super::KEYCHAIN_SERVICE_AGENT_OPENAI;
use super::KEYCHAIN_SERVICE_LLM_QUERY;
use super::KEYCHAIN_SERVICE_SEMANTIC_SCHOLAR;
use super::SETTINGS_SCHEMA_VERSION;

// ─── Default configs ────────────────────────────────────────────────

pub(crate) fn default_global_settings() -> Value {
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
          "domainConfig": {
            "domain": "general",
            "customInstructions": null,
            "terminologyStrictness": "moderate"
          },
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

pub(crate) fn default_project_settings() -> Value {
    json!({})
}

pub(crate) fn default_secret_settings() -> Value {
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

pub(crate) fn default_global_envelope() -> SettingsEnvelope {
    SettingsEnvelope {
        version: SETTINGS_SCHEMA_VERSION,
        data: sanitize_global_settings(&default_global_settings()),
    }
}

pub(crate) fn default_project_envelope() -> SettingsEnvelope {
    SettingsEnvelope {
        version: SETTINGS_SCHEMA_VERSION,
        data: sanitize_project_settings(&default_project_settings()),
    }
}

pub(crate) fn default_secret_envelope() -> SettingsEnvelope {
    SettingsEnvelope {
        version: SETTINGS_SCHEMA_VERSION,
        data: sanitize_secret_settings(&default_secret_settings()),
    }
}

// ─── Getter helpers ─────────────────────────────────────────────────

pub(crate) fn as_object(value: &Value) -> Option<&Map<String, Value>> {
    match value {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

pub(crate) fn get_in<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        let obj = as_object(current)?;
        current = obj.get(*segment)?;
    }
    Some(current)
}

pub(crate) fn get_number_in_range(value: Option<&Value>, min: f64, max: f64) -> Option<f64> {
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

pub(crate) fn get_bool(value: Option<&Value>, fallback: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(fallback)
}

pub(crate) fn get_enum(value: Option<&Value>, allowed: &[&str], fallback: &str) -> String {
    if let Some(s) = value.and_then(Value::as_str) {
        if allowed.contains(&s) {
            return s.to_string();
        }
    }
    fallback.to_string()
}

pub(crate) fn get_string_or_null(value: Option<&Value>) -> Value {
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

pub(crate) fn get_string_or(value: Option<&Value>, fallback: &str) -> String {
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

// ─── Legacy migration ───────────────────────────────────────────────

pub(crate) fn legacy_global_data(raw: &Value) -> Value {
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

// ─── Sanitization ───────────────────────────────────────────────────

pub(crate) fn sanitize_global_settings(input: &Value) -> Value {
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
    let agent_domain = get_enum(
        get_in(input, &["integrations", "agent", "domainConfig", "domain"]),
        &["general", "biomedical", "chemistry", "custom"],
        "general",
    );
    let agent_custom_instructions = get_in(
        input,
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
    let agent_terminology_strictness = get_enum(
        get_in(
            input,
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
          "domainConfig": {
            "domain": agent_domain,
            "customInstructions": agent_custom_instructions,
            "terminologyStrictness": agent_terminology_strictness
          },
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

pub(crate) fn sanitize_project_settings(input: &Value) -> Value {
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

pub(crate) fn sanitize_secret_settings(input: &Value) -> Value {
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

// ─── Migration ──────────────────────────────────────────────────────

pub(crate) fn merge_global_migration_source(raw: &Value) -> Value {
    let mut source = raw.clone();
    let legacy = legacy_global_data(raw);
    super::merge_value(&mut source, &legacy);
    source
}

pub(crate) fn migrate_global_envelope(raw: &Value) -> SettingsEnvelope {
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

pub(crate) fn migrate_project_envelope(raw: &Value) -> SettingsEnvelope {
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

pub(crate) fn migrate_secret_envelope(raw: &Value) -> SettingsEnvelope {
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

// ─── Resolution ─────────────────────────────────────────────────────

pub(crate) fn resolve_effective_settings(global_input: &Value, project_input: &Value) -> Value {
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
          "domainConfig": {
            "domain": "general",
            "customInstructions": null,
            "terminologyStrictness": "moderate"
          },
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

pub(crate) fn to_secrets_meta(secret_input: &Value) -> Value {
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

pub(crate) fn is_canonical_envelope_data(raw: &Value, migrated: &SettingsEnvelope) -> bool {
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
