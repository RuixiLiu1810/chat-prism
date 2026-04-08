use serde_json::{Map, Value};

use super::types::SettingsFieldError;

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

pub(crate) fn validate_global_patch(patch: &Value) -> Vec<SettingsFieldError> {
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
                                            "domainConfig" => {
                                                if let Some(domain_cfg) = validate_object(
                                                    av,
                                                    "integrations.agent.domainConfig",
                                                    &mut errors,
                                                ) {
                                                    for (dk, dv) in domain_cfg {
                                                        match dk.as_str() {
                                                            "domain" => validate_enum(
                                                                dv,
                                                                "integrations.agent.domainConfig.domain",
                                                                &[
                                                                    "general",
                                                                    "biomedical",
                                                                    "chemistry",
                                                                    "custom",
                                                                ],
                                                                &mut errors,
                                                            ),
                                                            "customInstructions" => {
                                                                if !(dv.is_string()
                                                                    || dv.is_null())
                                                                {
                                                                    push_error(
                                                                        &mut errors,
                                                                        "integrations.agent.domainConfig.customInstructions",
                                                                        "must be a string or null",
                                                                    );
                                                                }
                                                            }
                                                            "terminologyStrictness" => validate_enum(
                                                                dv,
                                                                "integrations.agent.domainConfig.terminologyStrictness",
                                                                &[
                                                                    "strict",
                                                                    "moderate",
                                                                    "relaxed",
                                                                ],
                                                                &mut errors,
                                                            ),
                                                            _ => push_error(
                                                                &mut errors,
                                                                "integrations.agent.domainConfig",
                                                                "unknown field",
                                                            ),
                                                        }
                                                    }
                                                }
                                            }
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

pub(crate) fn validate_project_patch(patch: &Value) -> Vec<SettingsFieldError> {
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

pub(crate) fn validate_secret_patch(patch: &Value) -> Vec<SettingsFieldError> {
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
