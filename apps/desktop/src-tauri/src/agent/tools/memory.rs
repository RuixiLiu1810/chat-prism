use chrono::Utc;
use serde_json::Value;
use tokio::sync::watch;
use uuid::Uuid;

use super::{AgentRuntimeState, AgentToolResult, error_result, ok_result, tool_arg_string};
use crate::agent::session::{MemoryEntry, MemoryType};

pub(crate) async fn execute_remember_fact(
    runtime_state: &AgentRuntimeState,
    call_id: &str,
    args: Value,
    _cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    let content = match tool_arg_string(&args, "content") {
        Ok(value) => value,
        Err(message) => return error_result("remember_fact", call_id, message),
    };

    let memory_type_str = args
        .get("memory_type")
        .and_then(|v| v.as_str())
        .unwrap_or("reference");

    let memory_type = match memory_type_str {
        "user_preference" => MemoryType::UserPreference,
        "project_convention" => MemoryType::ProjectConvention,
        "correction" => MemoryType::Correction,
        _ => MemoryType::Reference,
    };

    let topic = args
        .get("topic")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let now = Utc::now().to_rfc3339();
    let entry = MemoryEntry {
        id: Uuid::new_v4().to_string(),
        memory_type,
        content: content.clone(),
        topic,
        source_session: None,
        created_at: now.clone(),
        last_accessed: now,
    };

    match runtime_state.save_memory_entry(entry).await {
        Ok(()) => ok_result(
            "remember_fact",
            call_id,
            serde_json::json!({ "saved": true, "content": content }),
            format!("Remembered: {}", truncate(&content, 80)),
        ),
        Err(err) => error_result("remember_fact", call_id, err),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{}…", truncated)
    }
}
