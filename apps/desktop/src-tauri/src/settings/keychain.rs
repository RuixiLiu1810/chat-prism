use serde_json::Value;

use super::schema::get_in;
use super::KEYCHAIN_ACCOUNT;
use super::KEYCHAIN_SERVICE_AGENT_OPENAI;
use super::KEYCHAIN_SERVICE_LLM_QUERY;
use super::KEYCHAIN_SERVICE_SEMANTIC_SCHOLAR;

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

pub(crate) fn resolve_secret_value(secret: &Value, path: &[&str], service: &str) -> Option<String> {
    if let Some(value) = read_optional_secret(secret, path) {
        return Some(value);
    }
    keychain_read_secret(service).ok().flatten()
}

pub(crate) fn migrate_secret_values_to_keychain(
    secret: &mut Value,
    warnings: &mut Vec<String>,
) -> bool {
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

pub(crate) fn persist_secret_values_to_keychain_or_fallback(
    secret: &mut Value,
    warnings: &mut Vec<String>,
) {
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
