use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use agent_core::{
    AgentCompletePayload, AgentErrorEvent, AgentEventEnvelope, AgentEventPayload, AgentStatusEvent,
    AGENT_COMPLETE_EVENT_NAME, AGENT_EVENT_NAME, AGENT_PROTOCOL_VERSION,
};
use serde_json::Value;
use tauri::{Emitter, Manager, WebviewWindow};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
};

#[derive(Clone)]
pub struct LocalAgentProcessState {
    pub processes: Arc<Mutex<HashMap<String, LocalAgentProcessHandle>>>,
}

impl Default for LocalAgentProcessState {
    fn default() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

pub struct LocalAgentProcessHandle {
    child: Child,
    completion_emitted: Arc<AtomicBool>,
}

enum ParsedAgentLine {
    Event(AgentEventEnvelope),
    Complete(AgentCompletePayload),
    Unparsed(String),
}

fn process_key(window_label: &str, tab_id: &str) -> String {
    format!("{}:{}", window_label, tab_id)
}

fn fallback_session_id(tab_id: &str) -> String {
    format!("external-{}", tab_id)
}

fn non_empty_str(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn resolve_tab_id(value: &Value, fallback_tab_id: &str) -> String {
    value
        .get("tabId")
        .or_else(|| value.get("tab_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_tab_id)
        .to_string()
}

fn parse_agent_output_line(line: &str, fallback_tab_id: &str) -> ParsedAgentLine {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ParsedAgentLine::Unparsed(String::new());
    }

    let value: Value = match serde_json::from_str(trimmed) {
        Ok(value) => value,
        Err(_) => return ParsedAgentLine::Unparsed(line.to_string()),
    };

    if let Ok(mut envelope) = serde_json::from_value::<AgentEventEnvelope>(value.clone()) {
        if envelope.tab_id.trim().is_empty() {
            envelope.tab_id = fallback_tab_id.to_string();
        }
        return ParsedAgentLine::Event(envelope);
    }

    if let Ok(mut complete) = serde_json::from_value::<AgentCompletePayload>(value.clone()) {
        if complete.tab_id.trim().is_empty() {
            complete.tab_id = fallback_tab_id.to_string();
        }
        return ParsedAgentLine::Complete(complete);
    }

    if value.get("payload").is_some() {
        if let Some(payload_value) = value.get("payload") {
            if let Ok(payload) = serde_json::from_value::<AgentEventPayload>(payload_value.clone())
            {
                return ParsedAgentLine::Event(AgentEventEnvelope {
                    tab_id: resolve_tab_id(&value, fallback_tab_id),
                    payload,
                });
            }
        }
    }

    if let Some(outcome) = value.get("outcome").and_then(Value::as_str) {
        return ParsedAgentLine::Complete(AgentCompletePayload {
            tab_id: resolve_tab_id(&value, fallback_tab_id),
            outcome: outcome.to_string(),
            protocol_version: AGENT_PROTOCOL_VERSION,
        });
    }

    if let Ok(payload) = serde_json::from_value::<AgentEventPayload>(value) {
        return ParsedAgentLine::Event(AgentEventEnvelope {
            tab_id: fallback_tab_id.to_string(),
            payload,
        });
    }

    ParsedAgentLine::Unparsed(line.to_string())
}

fn emit_event(window: &WebviewWindow, envelope: AgentEventEnvelope) {
    let _ = window.emit(AGENT_EVENT_NAME, envelope);
}

fn emit_status(window: &WebviewWindow, tab_id: &str, stage: &str, message: impl Into<String>) {
    emit_event(
        window,
        AgentEventEnvelope {
            tab_id: tab_id.to_string(),
            payload: AgentEventPayload::Status(AgentStatusEvent {
                stage: stage.to_string(),
                message: message.into(),
            }),
        },
    );
}

fn emit_error(window: &WebviewWindow, tab_id: &str, code: &str, message: impl Into<String>) {
    emit_event(
        window,
        AgentEventEnvelope {
            tab_id: tab_id.to_string(),
            payload: AgentEventPayload::Error(AgentErrorEvent {
                code: code.to_string(),
                message: message.into(),
            }),
        },
    );
}

fn emit_complete(window: &WebviewWindow, tab_id: &str, outcome: &str) {
    let _ = window.emit(
        AGENT_COMPLETE_EVENT_NAME,
        AgentCompletePayload {
            tab_id: tab_id.to_string(),
            outcome: outcome.to_string(),
            protocol_version: AGENT_PROTOCOL_VERSION,
        },
    );
}

fn find_agent_runtime_binary() -> Result<PathBuf, String> {
    if let Some(path) = non_empty_str(std::env::var("PRISM_LOCAL_AGENT_BIN").ok().as_deref()) {
        let pb = PathBuf::from(path);
        if pb.exists() {
            return Ok(pb);
        }
        return Err(format!(
            "PRISM_LOCAL_AGENT_BIN points to a missing binary: {}",
            pb.display()
        ));
    }

    which::which("agent-runtime").map_err(|err| {
        format!(
            "Failed to locate external local agent binary 'agent-runtime': {}. Add it to PATH or set PRISM_LOCAL_AGENT_BIN.",
            err
        )
    })
}

fn build_local_agent_command(
    binary_path: &Path,
    project_path: &str,
    prompt: &str,
    tab_id: &str,
    model: Option<String>,
) -> Command {
    let mut cmd = Command::new(binary_path);
    cmd.arg("--project-path")
        .arg(project_path)
        .arg("--prompt")
        .arg(prompt)
        .arg("--tab-id")
        .arg(tab_id)
        .arg("--output")
        .arg("jsonl")
        .arg("--ui-mode")
        .arg("classic")
        .arg("--tool-mode")
        .arg("safe")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(model) = non_empty_str(model.as_deref()) {
        cmd.arg("--model").arg(model);
    }

    cmd
}

async fn spawn_local_agent_process(
    window: WebviewWindow,
    mut cmd: Command,
    tab_id: String,
) -> Result<(), String> {
    let window_label = window.label().to_string();
    let key = process_key(&window_label, &tab_id);

    let mut child = cmd
        .spawn()
        .map_err(|err| format!("Failed to spawn external local agent process: {}", err))?;

    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

    let completion_emitted = Arc::new(AtomicBool::new(false));

    let process_arc = window
        .state::<LocalAgentProcessState>()
        .inner()
        .processes
        .clone();

    {
        let mut processes = process_arc.lock().await;
        if let Some(mut existing) = processes.remove(&key) {
            existing.completion_emitted.store(true, Ordering::SeqCst);
            let _ = existing.child.kill().await;
        }
        processes.insert(
            key.clone(),
            LocalAgentProcessHandle {
                child,
                completion_emitted: Arc::clone(&completion_emitted),
            },
        );
    }

    let win_stdout = window.clone();
    let tab_stdout = tab_id.clone();
    let completion_stdout = Arc::clone(&completion_emitted);
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            match parse_agent_output_line(&line, &tab_stdout) {
                ParsedAgentLine::Event(envelope) => emit_event(&win_stdout, envelope),
                ParsedAgentLine::Complete(payload) => {
                    completion_stdout.store(true, Ordering::SeqCst);
                    let _ = win_stdout.emit(AGENT_COMPLETE_EVENT_NAME, payload);
                }
                ParsedAgentLine::Unparsed(raw) => {
                    if !raw.trim().is_empty() {
                        emit_status(&win_stdout, &tab_stdout, "external_stdout", raw);
                    }
                }
            }
        }
    });

    let win_stderr = window.clone();
    let tab_stderr = tab_id.clone();
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                emit_error(&win_stderr, &tab_stderr, "external_stderr", trimmed);
            }
        }
    });

    let win_wait = window;
    let tab_wait = tab_id;
    tokio::spawn(async move {
        let _ = stdout_task.await;
        let _ = stderr_task.await;

        let status = {
            let mut processes = process_arc.lock().await;
            processes.remove(&key).map(|mut handle| handle.child)
        };

        let (success, status_debug) = if let Some(mut child) = status {
            match child.wait().await {
                Ok(status) => (status.success(), Some(format!("{}", status))),
                Err(err) => (false, Some(format!("wait failed: {}", err))),
            }
        } else {
            (false, None)
        };

        if !completion_emitted.swap(true, Ordering::SeqCst) {
            if success {
                emit_complete(&win_wait, &tab_wait, "completed");
            } else {
                let detail = status_debug.unwrap_or_else(|| "process missing".to_string());
                emit_error(
                    &win_wait,
                    &tab_wait,
                    "external_exit",
                    format!("external local agent exited abnormally: {}", detail),
                );
                emit_complete(&win_wait, &tab_wait, "error");
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn execute_local_agent(
    window: WebviewWindow,
    project_path: String,
    prompt: String,
    tab_id: String,
    model: Option<String>,
) -> Result<String, String> {
    let binary = find_agent_runtime_binary()?;
    let cmd = build_local_agent_command(&binary, &project_path, &prompt, &tab_id, model);
    spawn_local_agent_process(window, cmd, tab_id.clone()).await?;
    Ok(fallback_session_id(&tab_id))
}

#[tauri::command]
pub async fn continue_local_agent(
    window: WebviewWindow,
    project_path: String,
    prompt: String,
    tab_id: String,
    model: Option<String>,
    local_session_id: Option<String>,
    _previous_response_id: Option<String>,
) -> Result<String, String> {
    let binary = find_agent_runtime_binary()?;
    let cmd = build_local_agent_command(&binary, &project_path, &prompt, &tab_id, model);
    spawn_local_agent_process(window, cmd, tab_id.clone()).await?;
    Ok(local_session_id.unwrap_or_else(|| fallback_session_id(&tab_id)))
}

#[tauri::command]
pub async fn resume_local_agent(
    window: WebviewWindow,
    project_path: String,
    _session_id: String,
    prompt: String,
    tab_id: String,
    model: Option<String>,
) -> Result<String, String> {
    let binary = find_agent_runtime_binary()?;
    let cmd = build_local_agent_command(&binary, &project_path, &prompt, &tab_id, model);
    spawn_local_agent_process(window, cmd, tab_id.clone()).await?;
    Ok(fallback_session_id(&tab_id))
}

#[tauri::command]
pub async fn cancel_local_agent(window: WebviewWindow, tab_id: String) -> Result<(), String> {
    let window_label = window.label().to_string();
    let key = process_key(&window_label, &tab_id);

    let state = window.state::<LocalAgentProcessState>();
    let mut processes = state.processes.lock().await;
    if let Some(mut handle) = processes.remove(&key) {
        handle.completion_emitted.store(true, Ordering::SeqCst);
        let _ = handle.child.kill().await;
        drop(processes);
        emit_complete(&window, &tab_id, "cancelled");
    }

    Ok(())
}

#[tauri::command]
pub async fn checkpoint_local_agent(
    window: WebviewWindow,
    tab_id: String,
    _local_session_id: Option<String>,
    decision: String,
    _feedback: Option<String>,
) -> Result<(), String> {
    emit_status(
        &window,
        &tab_id,
        "workflow_checkpoint",
        format!("Checkpoint decision received: {}", decision),
    );
    Ok(())
}

#[tauri::command]
pub async fn set_local_agent_tool_approval(
    window: WebviewWindow,
    tab_id: String,
    tool_name: String,
    decision: String,
) -> Result<(), String> {
    emit_status(
        &window,
        &tab_id,
        "approval_recorded",
        format!("{} => {}", tool_name, decision),
    );
    Ok(())
}

#[tauri::command]
pub async fn reset_local_agent_tool_approvals(
    window: WebviewWindow,
    tab_id: String,
) -> Result<(), String> {
    emit_status(&window, &tab_id, "approval_reset", "Tool approvals reset.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_agent_output_line, ParsedAgentLine};

    #[test]
    fn parses_agent_event_envelope_line() {
        let line = r#"{"tabId":"tab-1","payload":{"type":"status","stage":"thinking","message":"Planning"}}"#;
        let parsed = parse_agent_output_line(line, "fallback");
        match parsed {
            ParsedAgentLine::Event(envelope) => {
                assert_eq!(envelope.tab_id, "tab-1");
                let value = serde_json::to_value(envelope.payload)
                    .unwrap_or_else(|e| panic!("payload serialize: {e}"));
                assert_eq!(value["type"], "status");
            }
            _ => panic!("expected event"),
        }
    }

    #[test]
    fn parses_complete_payload_with_snake_case_tab_id() {
        let line = r#"{"tab_id":"tab-2","outcome":"completed"}"#;
        let parsed = parse_agent_output_line(line, "fallback");
        match parsed {
            ParsedAgentLine::Complete(payload) => {
                assert_eq!(payload.tab_id, "tab-2");
                assert_eq!(payload.outcome, "completed");
            }
            _ => panic!("expected completion"),
        }
    }

    #[test]
    fn wraps_payload_only_line_with_fallback_tab() {
        let line = r#"{"type":"status","stage":"streaming","message":"Connected"}"#;
        let parsed = parse_agent_output_line(line, "fallback-tab");
        match parsed {
            ParsedAgentLine::Event(envelope) => {
                assert_eq!(envelope.tab_id, "fallback-tab");
                let value = serde_json::to_value(envelope.payload)
                    .unwrap_or_else(|e| panic!("payload serialize: {e}"));
                assert_eq!(value["stage"], "streaming");
            }
            _ => panic!("expected wrapped event"),
        }
    }

    #[test]
    fn invalid_json_line_is_unparsed() {
        let parsed = parse_agent_output_line("not json", "fallback");
        match parsed {
            ParsedAgentLine::Unparsed(raw) => assert_eq!(raw, "not json"),
            _ => panic!("expected unparsed line"),
        }
    }
}
