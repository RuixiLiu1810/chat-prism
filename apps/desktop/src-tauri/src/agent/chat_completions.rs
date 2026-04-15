use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::watch;

use crate::settings;

use super::adapter::TauriConfigProvider;
use super::provider::{AgentStatus, AgentTurnDescriptor};
use super::session::AgentRuntimeState;
use super::tools::execute_tool_call;
use agent_core::{
    extract_text_segments, provider_display_name, provider_supports_transport, AgentTurnOutcome,
    ConfigProvider, EventSink, NullEventSink, ToolExecutorFn,
};

pub fn runtime_status(app: &tauri::AppHandle, provider: &str) -> AgentStatus {
    match settings::load_agent_runtime(app, None) {
        Ok(config) if config.provider == provider => {
            let configured = config
                .api_key
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
            let transport_ready = configured && provider_supports_transport(provider);
            AgentStatus {
                provider: provider.to_string(),
                display_name: provider_display_name(provider).to_string(),
                ready: transport_ready,
                mode: if transport_ready {
                    "chat_completions_ready".to_string()
                } else if configured {
                    "chat_completions_unavailable".to_string()
                } else {
                    "not_configured".to_string()
                },
                message: if transport_ready {
                    format!(
                        "{} is configured and smoke-validated for chat completions agent runtime.",
                        provider_display_name(provider)
                    )
                } else if configured {
                    format!(
                        "{} is configured, but this provider is not promoted to a working transport in the current runtime.",
                        provider_display_name(provider)
                    )
                } else {
                    format!(
                        "{} is selected, but the API key is not configured yet.",
                        provider_display_name(provider)
                    )
                },
                default_model: Some(config.model),
            }
        }
        Ok(config) => AgentStatus {
            provider: provider.to_string(),
            display_name: provider_display_name(provider).to_string(),
            ready: false,
            mode: "provider_mismatch".to_string(),
            message: format!(
                "{} is not the active agent provider. Current provider: {}.",
                provider_display_name(provider),
                config.provider
            ),
            default_model: Some(config.model),
        },
        Err(message) => AgentStatus {
            provider: provider.to_string(),
            display_name: provider_display_name(provider).to_string(),
            ready: false,
            mode: "not_configured".to_string(),
            message,
            default_model: None,
        },
    }
}

pub async fn run_turn_loop(
    sink: &dyn EventSink,
    config_provider: &dyn ConfigProvider,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    history: &[Value],
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<AgentTurnOutcome, String> {
    let executor_state = runtime_state.clone();
    let tab_id = request.tab_id.clone();
    let project_path = request.project_path.clone();
    let tool_executor: ToolExecutorFn = Arc::new(move |call, cancel_rx| {
        let runtime_state = executor_state.clone();
        let tab_id = tab_id.clone();
        let project_path = project_path.clone();
        Box::pin(async move {
            execute_tool_call(&runtime_state, &tab_id, &project_path, call, cancel_rx).await
        })
    });

    agent_core::providers::chat_completions::run_turn_loop(
        sink,
        config_provider,
        runtime_state,
        request,
        history,
        tool_executor,
        cancel_rx,
    )
    .await
}

async fn smoke_text_round(
    app: &tauri::AppHandle,
    project_path: &str,
    provider: &str,
) -> Result<(Vec<Value>, String), String> {
    let request = AgentTurnDescriptor {
        project_path: project_path.to_string(),
        prompt: "Reply with exactly the word READY and nothing else.".to_string(),
        tab_id: "smoke-text".to_string(),
        model: None,
        local_session_id: None,
        previous_response_id: None,
        turn_profile: None,
    };

    let history: Vec<Value> = Vec::new();
    let outcome = match provider {
        "minimax" | "deepseek" => run_turn_loop_silent(app, &request, &history).await?,
        other => {
            return Err(format!(
                "{} smoke test is not wired yet.",
                provider_display_name(other)
            ));
        }
    };

    let reply = outcome
        .messages
        .iter()
        .rev()
        .find(|message| message.get("type").and_then(Value::as_str) == Some("assistant"))
        .map(extract_text_segments)
        .unwrap_or_default()
        .join("\n\n");

    if !reply.to_uppercase().contains("READY") {
        return Err(format!(
            "Text round completed, but the reply did not contain READY: {}",
            reply
        ));
    }

    Ok((outcome.messages, reply))
}

async fn smoke_tool_round(
    app: &tauri::AppHandle,
    project_path: &str,
    provider: &str,
) -> Result<String, String> {
    let request = AgentTurnDescriptor {
        project_path: project_path.to_string(),
        prompt: "Use the list_files tool on the project root, then answer with the first 3 filenames you found in one short line.".to_string(),
        tab_id: "smoke-tool".to_string(),
        model: None,
        local_session_id: None,
        previous_response_id: None,
        turn_profile: None,
    };

    let history: Vec<Value> = Vec::new();
    let outcome = match provider {
        "minimax" | "deepseek" => run_turn_loop_silent(app, &request, &history).await?,
        other => {
            return Err(format!(
                "{} smoke test is not wired yet.",
                provider_display_name(other)
            ));
        }
    };

    let tool_result_count = outcome
        .messages
        .iter()
        .filter(|message| {
            message
                .pointer("/message/content")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .any(|item| item.get("type").and_then(Value::as_str) == Some("tool_result"))
                })
                .unwrap_or(false)
        })
        .count();

    if tool_result_count == 0 {
        return Err(
            "Tool round completed, but no visible tool_result message was recorded.".to_string(),
        );
    }

    let reply = outcome
        .messages
        .iter()
        .rev()
        .find(|message| message.get("type").and_then(Value::as_str) == Some("assistant"))
        .map(extract_text_segments)
        .unwrap_or_default()
        .join("\n\n");

    Ok(reply)
}

async fn smoke_continuation_round(
    app: &tauri::AppHandle,
    project_path: &str,
    provider: &str,
) -> Result<String, String> {
    let first_request = AgentTurnDescriptor {
        project_path: project_path.to_string(),
        prompt: "Remember this project code: PRISM-SMOKE-17. Reply with exactly REMEMBERED."
            .to_string(),
        tab_id: "smoke-cont-1".to_string(),
        model: None,
        local_session_id: None,
        previous_response_id: None,
        turn_profile: None,
    };
    let first_outcome = match provider {
        "minimax" | "deepseek" => run_turn_loop_silent(app, &first_request, &[]).await?,
        other => {
            return Err(format!(
                "{} smoke test is not wired yet.",
                provider_display_name(other)
            ));
        }
    };

    let second_request = AgentTurnDescriptor {
        project_path: project_path.to_string(),
        prompt: "What project code did I ask you to remember? Reply with the code only."
            .to_string(),
        tab_id: "smoke-cont-2".to_string(),
        model: None,
        local_session_id: None,
        previous_response_id: None,
        turn_profile: None,
    };
    let second_outcome = match provider {
        "minimax" | "deepseek" => {
            run_turn_loop_silent(app, &second_request, &first_outcome.messages).await?
        }
        other => {
            return Err(format!(
                "{} smoke test is not wired yet.",
                provider_display_name(other)
            ));
        }
    };

    let reply = second_outcome
        .messages
        .iter()
        .rev()
        .find(|message| message.get("type").and_then(Value::as_str) == Some("assistant"))
        .map(extract_text_segments)
        .unwrap_or_default()
        .join("\n\n");

    if !reply.contains("PRISM-SMOKE-17") {
        return Err(format!(
            "Continuation round completed, but the reply lost context: {}",
            reply
        ));
    }

    Ok(reply)
}

async fn run_turn_loop_silent(
    app: &tauri::AppHandle,
    request: &AgentTurnDescriptor,
    history: &[Value],
) -> Result<AgentTurnOutcome, String> {
    let null_sink = NullEventSink;
    let runtime_state = AgentRuntimeState::default();
    let config_provider = TauriConfigProvider { app };
    run_turn_loop(
        &null_sink,
        &config_provider,
        &runtime_state,
        request,
        history,
        None,
    )
    .await
}

pub async fn smoke_test(
    app: &tauri::AppHandle,
    project_path: &str,
    provider: &str,
) -> Result<super::AgentSmokeResult, String> {
    let runtime = settings::load_agent_runtime(app, Some(project_path))?;
    let runtime_mode = if provider_supports_transport(provider) {
        "chat_completions".to_string()
    } else {
        "chat_completions_unavailable".to_string()
    };

    if runtime.provider != provider {
        return Err(format!(
            "{} is not the active agent provider. Current provider: {}.",
            provider_display_name(provider),
            runtime.provider
        ));
    }

    let mut steps = Vec::new();

    match smoke_text_round(app, project_path, provider).await {
        Ok((_history, reply)) => steps.push(super::AgentSmokeStep {
            name: "text_stream".to_string(),
            ok: true,
            detail: format!("Received streamed reply: {}", reply),
        }),
        Err(err) => {
            steps.push(super::AgentSmokeStep {
                name: "text_stream".to_string(),
                ok: false,
                detail: err,
            });
            return Ok(super::AgentSmokeResult {
                provider: provider.to_string(),
                runtime_mode,
                ok: false,
                steps,
            });
        }
    }

    match smoke_tool_round(app, project_path, provider).await {
        Ok(reply) => steps.push(super::AgentSmokeStep {
            name: "tool_loop".to_string(),
            ok: true,
            detail: format!("Tool loop completed: {}", reply),
        }),
        Err(err) => {
            steps.push(super::AgentSmokeStep {
                name: "tool_loop".to_string(),
                ok: false,
                detail: err,
            });
            return Ok(super::AgentSmokeResult {
                provider: provider.to_string(),
                runtime_mode,
                ok: false,
                steps,
            });
        }
    }

    match smoke_continuation_round(app, project_path, provider).await {
        Ok(reply) => steps.push(super::AgentSmokeStep {
            name: "continuation".to_string(),
            ok: true,
            detail: format!("Continuation preserved context: {}", reply),
        }),
        Err(err) => {
            steps.push(super::AgentSmokeStep {
                name: "continuation".to_string(),
                ok: false,
                detail: err,
            });
            return Ok(super::AgentSmokeResult {
                provider: provider.to_string(),
                runtime_mode,
                ok: false,
                steps,
            });
        }
    }

    Ok(super::AgentSmokeResult {
        provider: provider.to_string(),
        runtime_mode,
        ok: true,
        steps,
    })
}

pub async fn cancel_response(app: &tauri::AppHandle, provider: &str) -> Result<(), String> {
    let config = settings::load_agent_runtime(app, None)?;
    let _client = Client::new();
    Err(format!(
        "{} does not support cancel in the current local runtime yet (active provider: {}).",
        provider_display_name(provider),
        config.provider
    ))
}
