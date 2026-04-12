use std::collections::BTreeMap;
use std::time::Instant;

use reqwest::Client;
use serde_json::{json, Value};
use tauri::{Manager, WebviewWindow};
use tokio::sync::watch;

use crate::settings;

use super::openai::AgentTurnOutcome;
use super::provider::AgentSamplingProfile;
use super::session::AgentRuntimeState;
use super::telemetry::{
    document_artifact_miss, document_fallback_used, record_document_question_metrics,
};
use super::tools::{
    default_tool_specs, is_document_tool_name, to_chat_completions_tool_schema, AgentToolCall,
};
use super::turn_engine::{
    compact_chat_messages, emit_error, emit_status, emit_text_delta, execute_tool_calls,
    should_surface_assistant_text, tool_result_feedback_for_model,
    tool_result_has_invalid_arguments_error, ToolCallTracker, TurnBudget,
};
use super::{
    agent_instructions_for_request, max_rounds_for_task, resolve_turn_profile, tool_choice_for_task,
};
use super::{AgentStatus, AgentTurnDescriptor, AGENT_CANCELLED_MESSAGE};

#[derive(Debug, Clone, Default)]
struct ChatCompletionsToolCallBuilder {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Default)]
struct ChatCompletionsAssistantMessage {
    content: String,
    reasoning_details: Vec<Value>,
    tool_calls: Vec<Value>,
}

#[derive(Debug, Clone)]
struct StreamChatOutcome {
    assistant_message: ChatCompletionsAssistantMessage,
    tool_calls: Vec<AgentToolCall>,
}

pub fn provider_display_name(provider: &str) -> &'static str {
    match provider {
        "minimax" => "MiniMax Chat Completions",
        "deepseek" => "DeepSeek Chat Completions",
        _ => "Chat Completions",
    }
}

const TOOL_ARGUMENTS_RETRY_HINT: &str = "[Tool argument recovery rule]\n\
The previous tool call arguments were invalid JSON. Retry by emitting tool arguments as a strict JSON object only (no markdown fences, no prose, no trailing commentary). Include every required field.";

fn provider_supports_required_tool_choice(provider: &str) -> bool {
    matches!(provider, "minimax")
}

fn effective_tool_choice_for_provider<'a>(provider: &str, requested: &'a str) -> (&'a str, bool) {
    if requested == "required" && !provider_supports_required_tool_choice(provider) {
        ("auto", true)
    } else {
        (requested, false)
    }
}

fn provider_supports_transport(provider: &str) -> bool {
    matches!(provider, "minimax" | "deepseek")
}

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

fn parse_sse_frame(frame: &str) -> Option<(String, String)> {
    let mut event_name: Option<String> = None;
    let mut data_lines = Vec::new();

    for raw_line in frame.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("event:") {
            event_name = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }

    if data_lines.is_empty() {
        return None;
    }

    Some((
        event_name.unwrap_or_else(|| "message".to_string()),
        data_lines.join("\n"),
    ))
}

fn take_next_sse_frame(buffer: &mut String) -> Option<(String, String)> {
    let ends = [
        buffer.find("\r\n\r\n").map(|idx| (idx, 4)),
        buffer.find("\n\n").map(|idx| (idx, 2)),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|(idx, _)| *idx);

    let (idx, sep_len) = ends?;
    let frame = buffer[..idx].to_string();
    buffer.drain(..idx + sep_len);
    parse_sse_frame(&frame)
}

fn merge_stream_fragment(existing: &str, incoming: &str) -> String {
    if incoming.is_empty() {
        return String::new();
    }
    if existing.is_empty() {
        return incoming.to_string();
    }
    if incoming.starts_with(existing) {
        return incoming[existing.len()..].to_string();
    }
    incoming.to_string()
}

fn push_reasoning_delta(reasoning_details: &mut Vec<Value>, delta: &Value) {
    let Some(items) = delta.get("reasoning_details").and_then(Value::as_array) else {
        return;
    };

    while reasoning_details.len() < items.len() {
        reasoning_details.push(json!({}));
    }

    for (index, item) in items.iter().enumerate() {
        let Some(item_obj) = item.as_object() else {
            continue;
        };
        let target = reasoning_details[index]
            .as_object_mut()
            .expect("reasoning detail should stay object");

        for (key, value) in item_obj {
            if key == "text" {
                let existing = target
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let incoming = value.as_str().unwrap_or_default();
                let merged = if existing.is_empty() {
                    incoming.to_string()
                } else {
                    let delta = merge_stream_fragment(existing, incoming);
                    format!("{}{}", existing, delta)
                };
                target.insert("text".to_string(), Value::String(merged));
            } else {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

fn visible_text_message(role: &str, text: &str) -> Value {
    json!({
        "type": role,
        "message": {
            "content": [
                {
                    "type": "text",
                    "text": text,
                }
            ]
        }
    })
}

fn visible_assistant_message(text: &str, tool_calls: &[AgentToolCall]) -> Value {
    let mut content = Vec::new();
    if should_surface_assistant_text(text, tool_calls) {
        content.push(json!({
            "type": "text",
            "text": text,
        }));
    }
    for call in tool_calls {
        let parsed_input =
            serde_json::from_str::<Value>(&call.arguments).unwrap_or_else(|_| json!({}));
        content.push(json!({
            "type": "tool_use",
            "id": call.call_id,
            "name": call.tool_name,
            "input": parsed_input,
        }));
    }
    json!({
        "type": "assistant",
        "message": {
            "content": content,
        }
    })
}

fn visible_tool_result_message(call_id: &str, preview: &str, is_error: bool) -> Value {
    json!({
        "type": "user",
        "message": {
            "content": [
                {
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": preview,
                    "is_error": is_error,
                }
            ]
        }
    })
}

fn hidden_chat_message(message: Value) -> Value {
    json!({
        "type": "chat_message",
        "message": message,
    })
}

fn extract_text_segments(message: &Value) -> Vec<String> {
    message
        .pointer("/message/content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let kind = item.get("type").and_then(Value::as_str)?;
                    match kind {
                        "text" => item.get("text").and_then(Value::as_str).map(str::to_string),
                        "tool_result" => item
                            .get("content")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        _ => None,
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn extract_text_blocks_only(message: &Value) -> Vec<String> {
    message
        .pointer("/message/content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if item.get("type").and_then(Value::as_str) != Some("text") {
                        return None;
                    }
                    item.get("text").and_then(Value::as_str).map(str::to_string)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn extract_tool_use_blocks(message: &Value) -> Vec<Value> {
    message
        .pointer("/message/content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if item.get("type").and_then(Value::as_str) != Some("tool_use") {
                        return None;
                    }
                    let input = item.get("input").cloned().unwrap_or_else(|| json!({}));
                    Some(json!({
                        "id": item.get("id").cloned().unwrap_or(Value::Null),
                        "type": "function",
                        "function": {
                            "name": item.get("name").cloned().unwrap_or(Value::Null),
                            "arguments": serde_json::to_string(&input)
                                .unwrap_or_else(|_| "{}".to_string()),
                        }
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn extract_tool_result_blocks(message: &Value) -> Vec<Value> {
    message
        .pointer("/message/content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if item.get("type").and_then(Value::as_str) != Some("tool_result") {
                        return None;
                    }
                    Some(json!({
                        "role": "tool",
                        "tool_call_id": item.get("tool_use_id").cloned().unwrap_or(Value::Null),
                        "content": item.get("content").and_then(Value::as_str).unwrap_or_default(),
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn sampling_profile_params(
    profile: Option<&AgentSamplingProfile>,
    config: Option<&settings::AgentSamplingProfilesConfig>,
) -> Option<(f64, f64, u32)> {
    match profile {
        Some(AgentSamplingProfile::EditStable) => config
            .map(|profiles| {
                (
                    profiles.edit_stable.temperature,
                    profiles.edit_stable.top_p,
                    profiles.edit_stable.max_tokens,
                )
            })
            .or(Some((0.2, 0.9, 8192))),
        Some(AgentSamplingProfile::AnalysisBalanced) => config
            .map(|profiles| {
                (
                    profiles.analysis_balanced.temperature,
                    profiles.analysis_balanced.top_p,
                    profiles.analysis_balanced.max_tokens,
                )
            })
            .or(Some((0.4, 0.9, 6144))),
        Some(AgentSamplingProfile::AnalysisDeep) => config
            .map(|profiles| {
                (
                    profiles.analysis_deep.temperature,
                    profiles.analysis_deep.top_p,
                    profiles.analysis_deep.max_tokens,
                )
            })
            .or(Some((0.3, 0.92, 12288))),
        Some(AgentSamplingProfile::ChatFlexible) => config
            .map(|profiles| {
                (
                    profiles.chat_flexible.temperature,
                    profiles.chat_flexible.top_p,
                    profiles.chat_flexible.max_tokens,
                )
            })
            .or(Some((0.7, 0.95, 4096))),
        _ => None,
    }
}

pub fn transcript_to_chat_messages(
    instructions: &str,
    request: &AgentTurnDescriptor,
    history: &[Value],
) -> Vec<Value> {
    let has_raw_chat_entries = history
        .iter()
        .any(|item| item.get("type").and_then(Value::as_str) == Some("chat_message"));

    let mut messages = vec![json!({
        "role": "system",
        "content": instructions,
    })];

    for item in history {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
        if has_raw_chat_entries {
            if item_type == "chat_message" {
                if let Some(message) = item.get("message") {
                    messages.push(message.clone());
                }
            }
            continue;
        }

        match item_type {
            "assistant" => {
                let content = extract_text_segments(item).join("\n\n");
                let tool_calls = extract_tool_use_blocks(item);
                if content.trim().is_empty() && tool_calls.is_empty() {
                    continue;
                }
                let mut message = json!({
                    "role": "assistant",
                    "content": if content.trim().is_empty() {
                        Value::Null
                    } else {
                        Value::String(content)
                    },
                });
                if !tool_calls.is_empty() {
                    message["tool_calls"] = Value::Array(tool_calls);
                }
                messages.push(message);
            }
            "user" => {
                let content = extract_text_blocks_only(item).join("\n\n");
                if !content.trim().is_empty() {
                    messages.push(json!({
                        "role": "user",
                        "content": content,
                    }));
                }
                messages.extend(extract_tool_result_blocks(item));
            }
            _ => {}
        }
    }

    messages.push(json!({
        "role": "user",
        "content": request.prompt,
    }));

    messages
}

#[cfg(test)]
mod text_surface_tests {
    use super::should_surface_assistant_text;
    use crate::agent::tools::AgentToolCall;

    #[test]
    fn hides_visible_assistant_prose_when_edit_tool_is_invoked() {
        let tool_calls = vec![AgentToolCall {
            tool_name: "replace_selected_text".to_string(),
            call_id: "call_1".to_string(),
            arguments: "{}".to_string(),
        }];
        assert!(!should_surface_assistant_text(
            "Refined paragraph changes...",
            &tool_calls
        ));
    }

    #[test]
    fn keeps_visible_assistant_prose_for_non_write_tools() {
        let tool_calls = vec![AgentToolCall {
            tool_name: "read_file".to_string(),
            call_id: "call_1".to_string(),
            arguments: "{}".to_string(),
        }];
        assert!(should_surface_assistant_text(
            "I need to inspect the file first.",
            &tool_calls
        ));
    }
}

fn validate_transport_runtime(
    app: &tauri::AppHandle,
    provider: &str,
    project_root: Option<&str>,
) -> Result<settings::AgentRuntimeConfig, String> {
    let config = settings::load_agent_runtime(app, project_root)?;
    if config.provider != provider {
        return Err(format!(
            "{} is not the active provider. Current provider: {}.",
            provider_display_name(provider),
            config.provider
        ));
    }
    if !provider_supports_transport(provider) {
        return Err(format!(
            "{} is not promoted to a working transport in the current runtime.",
            provider_display_name(provider)
        ));
    }
    if config
        .api_key
        .as_ref()
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
    {
        return Err(format!(
            "{} API key is not configured.",
            provider_display_name(provider)
        ));
    }
    Ok(config)
}

async fn stream_chat_completions_response_once(
    window: Option<&WebviewWindow>,
    app: &tauri::AppHandle,
    request: &AgentTurnDescriptor,
    provider: &str,
    messages: Vec<Value>,
    mut cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<StreamChatOutcome, String> {
    let config = validate_transport_runtime(app, provider, Some(&request.project_path))?;
    let api_key = config
        .api_key
        .clone()
        .ok_or_else(|| "MiniMax API key is missing.".to_string())?;
    let model = request
        .model
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(config.model);
    let resolved_profile = resolve_turn_profile(request);
    let requested_tool_choice = tool_choice_for_task(request, &resolved_profile);
    let (effective_tool_choice, _) =
        effective_tool_choice_for_provider(&config.provider, requested_tool_choice);

    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "tools": default_tool_specs()
            .iter()
            .map(|spec| to_chat_completions_tool_schema(spec, &config.provider))
            .collect::<Vec<_>>(),
        "tool_choice": effective_tool_choice,
    });
    if config.provider == "minimax" {
        body["reasoning_split"] = Value::Bool(true);
    }
    if let Some((temperature, top_p, max_tokens)) = sampling_profile_params(
        Some(&resolved_profile.sampling_profile),
        Some(&config.sampling_profiles),
    ) {
        body["temperature"] = json!(temperature);
        body["top_p"] = json!(top_p);
        body["max_tokens"] = json!(max_tokens);
    }

    let client = Client::new();
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let response = client
        .post(url)
        .bearer_auth(api_key)
        .header("Accept", "text/event-stream")
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|err| {
            format!(
                "{} request failed: {}",
                provider_display_name(provider),
                err
            )
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let preview = if body.len() > 500 {
            format!("{}...", &body[..500])
        } else {
            body
        };
        return Err(format!(
            "{} request failed with status {}: {}",
            provider_display_name(provider),
            status,
            preview
        ));
    }

    emit_status(
        window,
        &request.tab_id,
        "streaming",
        &format!("Connected to {}.", provider_display_name(provider)),
    );

    let mut response = response;
    let mut buffer = String::new();
    let mut assistant_content = String::new();
    let mut reasoning_details = Vec::new();
    let mut tool_call_builders: BTreeMap<usize, ChatCompletionsToolCallBuilder> = BTreeMap::new();

    loop {
        let next_chunk = if let Some(cancel_rx) = cancel_rx.as_mut() {
            tokio::select! {
                changed = cancel_rx.changed() => {
                    match changed {
                        Ok(_) if *cancel_rx.borrow() => return Err(AGENT_CANCELLED_MESSAGE.to_string()),
                        Ok(_) => continue,
                        Err(_) => return Err(AGENT_CANCELLED_MESSAGE.to_string()),
                    }
                }
                chunk = response.chunk() => chunk.map_err(|err| {
                    format!(
                        "{} streaming read failed: {}",
                        provider_display_name(provider),
                        err
                    )
                })?
            }
        } else {
            response.chunk().await.map_err(|err| {
                format!(
                    "{} streaming read failed: {}",
                    provider_display_name(provider),
                    err
                )
            })?
        };

        let Some(chunk) = next_chunk else {
            break;
        };
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some((_event_name, data)) = take_next_sse_frame(&mut buffer) {
            if data == "[DONE]" {
                continue;
            }

            let parsed: Value = match serde_json::from_str(&data) {
                Ok(value) => value,
                Err(err) => {
                    emit_error(
                        window,
                        &request.tab_id,
                        "agent_stream_parse_error",
                        format!("Failed to parse MiniMax streaming payload: {}", err),
                    );
                    continue;
                }
            };

            let choice = parsed
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|choices| choices.first())
                .cloned()
                .unwrap_or_else(|| json!({}));

            if let Some(delta) = choice.get("delta") {
                if let Some(content_text) = delta.get("content").and_then(Value::as_str) {
                    let delta_text = merge_stream_fragment(&assistant_content, content_text);
                    assistant_content.push_str(&delta_text);
                    if !delta_text.is_empty() {
                        emit_text_delta(window, &request.tab_id, &delta_text);
                    }
                }

                push_reasoning_delta(&mut reasoning_details, delta);

                if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for (fallback_index, tool_call) in tool_calls.iter().enumerate() {
                        let index = tool_call
                            .get("index")
                            .and_then(Value::as_u64)
                            .map(|value| value as usize)
                            .unwrap_or(fallback_index);
                        let builder = tool_call_builders.entry(index).or_default();
                        if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                            builder.id = id.to_string();
                        }
                        if let Some(function) = tool_call.get("function") {
                            if let Some(name) = function.get("name").and_then(Value::as_str) {
                                builder.name = name.to_string();
                            }
                            if let Some(arguments) =
                                function.get("arguments").and_then(Value::as_str)
                            {
                                builder.arguments = if builder.arguments.is_empty() {
                                    arguments.to_string()
                                } else {
                                    format!(
                                        "{}{}",
                                        builder.arguments,
                                        merge_stream_fragment(&builder.arguments, arguments)
                                    )
                                };
                            }
                        }
                    }
                }
            }

            if let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) {
                if matches!(finish_reason, "stop" | "tool_calls") {
                    emit_status(
                        window,
                        &request.tab_id,
                        "completed",
                        &format!("{} response completed.", provider_display_name(provider)),
                    );
                }
            }
        }
    }

    let tool_calls = tool_call_builders
        .into_values()
        .map(|builder| AgentToolCall {
            tool_name: builder.name.clone(),
            call_id: builder.id.clone(),
            arguments: if builder.arguments.trim().is_empty() {
                "{}".to_string()
            } else {
                builder.arguments.clone()
            },
        })
        .collect::<Vec<_>>();

    let raw_tool_calls = tool_calls
        .iter()
        .map(|call| {
            json!({
                "id": call.call_id,
                "type": "function",
                "function": {
                    "name": call.tool_name,
                    "arguments": call.arguments,
                }
            })
        })
        .collect::<Vec<_>>();

    Ok(StreamChatOutcome {
        assistant_message: ChatCompletionsAssistantMessage {
            content: assistant_content,
            reasoning_details,
            tool_calls: raw_tool_calls,
        },
        tool_calls,
    })
}

fn raw_assistant_message(payload: &ChatCompletionsAssistantMessage) -> Value {
    let mut message = json!({
        "role": "assistant",
        "content": if payload.content.is_empty() {
            Value::Null
        } else {
            Value::String(payload.content.clone())
        },
    });
    if !payload.tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(payload.tool_calls.clone());
    }
    if !payload.reasoning_details.is_empty() {
        message["reasoning_details"] = Value::Array(payload.reasoning_details.clone());
    }
    message
}

pub async fn run_turn_loop(
    window: &WebviewWindow,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    history: &[Value],
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<AgentTurnOutcome, String> {
    fn request_has_binary_attachment_context(prompt: &str) -> bool {
        prompt.lines().any(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with("[Resource path: ") || !trimmed.ends_with(']') {
                return false;
            }
            let path = trimmed
                .trim_start_matches("[Resource path: ")
                .trim_end_matches(']')
                .trim()
                .to_ascii_lowercase();
            path.ends_with(".pdf") || path.ends_with(".docx")
        })
    }

    runtime_state.ensure_storage(&window.app_handle()).await?;
    let runtime = settings::load_agent_runtime(&window.app_handle(), Some(&request.project_path))?;
    match runtime.provider.as_str() {
        "minimax" | "deepseek" => {}
        other => {
            return Err(format!(
                "{} cannot be handled by chat_completions runtime.",
                other
            ))
        }
    }

    let mut transcript_messages = vec![
        visible_text_message("user", &request.prompt),
        hidden_chat_message(json!({
            "role": "user",
            "content": request.prompt,
        })),
    ];
    let resolved_profile = resolve_turn_profile(request);
    let mut instructions =
        agent_instructions_for_request(runtime_state, request, Some(&runtime)).await;
    let requested_tool_choice = tool_choice_for_task(request, &resolved_profile);
    let (_, downgraded_tool_choice) =
        effective_tool_choice_for_provider(&runtime.provider, requested_tool_choice);
    if downgraded_tool_choice {
        instructions.push_str(
            "\n[Tool-calling fallback]\n\
            This provider may ignore tool_choice='required'. You MUST call at least one appropriate tool before finalizing the answer for this turn.\n",
        );
    }
    let mut next_messages = transcript_to_chat_messages(&instructions, request, history);
    compact_chat_messages(&mut next_messages);
    let turn_started_at = Instant::now();
    let mut doc_tool_rounds = 0u32;
    let mut doc_tool_calls = 0u32;
    let mut artifact_miss_count = 0u32;
    let mut fallback_count = 0u32;
    let is_document_question = request_has_binary_attachment_context(&request.prompt);
    let mut budget = TurnBudget::new(
        max_rounds_for_task(&resolved_profile),
        sampling_profile_params(
            Some(&resolved_profile.sampling_profile),
            Some(&runtime.sampling_profiles),
        )
        .map(|(_, _, max_tokens)| max_tokens),
        cancel_rx.clone(),
    );
    let mut tracker = ToolCallTracker::new(budget.max_rounds);

    for round_idx in 0..budget.max_rounds {
        tracker.current_round = round_idx;
        budget.ensure_round_available(round_idx)?;
        let outcome = stream_chat_completions_response_once(
            Some(window),
            &window.app_handle(),
            request,
            &runtime.provider,
            next_messages.clone(),
            budget.clone_abort_rx(),
        )
        .await?;
        budget.record_output_text(&outcome.assistant_message.content)?;
        let raw_assistant = raw_assistant_message(&outcome.assistant_message);

        transcript_messages.push(visible_assistant_message(
            &outcome.assistant_message.content,
            &outcome.tool_calls,
        ));
        transcript_messages.push(hidden_chat_message(raw_assistant.clone()));

        if outcome.tool_calls.is_empty() {
            if is_document_question || doc_tool_calls > 0 {
                record_document_question_metrics(
                    runtime_state,
                    request,
                    "completed",
                    doc_tool_rounds,
                    doc_tool_calls,
                    artifact_miss_count,
                    fallback_count,
                    turn_started_at.elapsed(),
                )
                .await;
            }
            return Ok(AgentTurnOutcome {
                response_id: None,
                messages: transcript_messages,
                suspended: false,
            });
        }

        let round_doc_calls = outcome
            .tool_calls
            .iter()
            .filter(|call| is_document_tool_name(&call.tool_name))
            .count() as u32;
        if round_doc_calls > 0 {
            doc_tool_rounds = doc_tool_rounds.saturating_add(1);
            doc_tool_calls = doc_tool_calls.saturating_add(round_doc_calls);
        }

        let mut tool_results_messages = vec![raw_assistant];
        let mut invalid_tool_arguments_detected = false;

        // Record calls in tracker before execution
        for call in &outcome.tool_calls {
            tracker.record_call(&call.tool_name, &call.arguments);
        }

        let executed_calls = execute_tool_calls(
            Some(window),
            runtime_state,
            request,
            outcome.tool_calls,
            budget.clone_abort_rx(),
        )
        .await;
        for executed in &executed_calls.executed {
            let result = executed.result.clone();
            if result.content.get("error").and_then(Value::as_str) == Some(AGENT_CANCELLED_MESSAGE)
            {
                if is_document_question || doc_tool_calls > 0 {
                    record_document_question_metrics(
                        runtime_state,
                        request,
                        "cancelled",
                        doc_tool_rounds,
                        doc_tool_calls,
                        artifact_miss_count,
                        fallback_count,
                        turn_started_at.elapsed(),
                    )
                    .await;
                }
                return Err(AGENT_CANCELLED_MESSAGE.to_string());
            }
            if is_document_tool_name(&result.tool_name) {
                if document_artifact_miss(&result) {
                    artifact_miss_count = artifact_miss_count.saturating_add(1);
                }
                if document_fallback_used(&result) {
                    fallback_count = fallback_count.saturating_add(1);
                }
            }
            if tool_result_has_invalid_arguments_error(&result) {
                invalid_tool_arguments_detected = true;
            }
            let feedback = tool_result_feedback_for_model(&result);
            budget.record_output_text(&feedback)?;
            transcript_messages.push(visible_tool_result_message(
                &result.call_id,
                &result.preview,
                result.is_error,
            ));
            tool_results_messages.push(json!({
                "role": "tool",
                "tool_call_id": result.call_id,
                "content": feedback,
            }));
            transcript_messages.push(hidden_chat_message(
                tool_results_messages
                    .last()
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            ));
        }

        if executed_calls.suspended {
            if is_document_question || doc_tool_calls > 0 {
                record_document_question_metrics(
                    runtime_state,
                    request,
                    "suspended",
                    doc_tool_rounds,
                    doc_tool_calls,
                    artifact_miss_count,
                    fallback_count,
                    turn_started_at.elapsed(),
                )
                .await;
            }
            return Ok(AgentTurnOutcome {
                response_id: None,
                messages: transcript_messages,
                suspended: true,
            });
        }

        next_messages.extend(tool_results_messages);
        compact_chat_messages(&mut next_messages);

        // Inject loop-guard warnings / progress checkpoint
        if let Some(injection) = tracker.build_injection(round_idx) {
            next_messages.push(json!({
                "role": "system",
                "content": injection,
            }));
        }

        if invalid_tool_arguments_detected {
            next_messages.push(json!({
                "role": "system",
                "content": TOOL_ARGUMENTS_RETRY_HINT,
            }));
            emit_status(
                Some(window),
                &request.tab_id,
                "tool_retry_hint",
                "Tool arguments were invalid. Retrying with strict JSON argument guidance.",
            );
        }
        emit_status(
            Some(window),
            &request.tab_id,
            "responding_after_tools",
            "Tool results sent back to MiniMax. Continuing...",
        );
    }

    if is_document_question || doc_tool_calls > 0 {
        record_document_question_metrics(
            runtime_state,
            request,
            "round_limit_exceeded",
            doc_tool_rounds,
            doc_tool_calls,
            artifact_miss_count,
            fallback_count,
            turn_started_at.elapsed(),
        )
        .await;
    }

    Err(format!(
        "{} tool loop exceeded {} rounds; aborting to avoid an infinite agent loop.",
        provider_display_name(&runtime.provider),
        max_rounds_for_task(&resolved_profile)
    ))
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
            ))
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
            ))
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
            ))
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
            ))
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
    let runtime_state = AgentRuntimeState::default();
    let runtime = settings::load_agent_runtime(app, Some(&request.project_path))?;
    match runtime.provider.as_str() {
        "minimax" | "deepseek" => {}
        other => {
            return Err(format!(
                "{} cannot be handled by chat_completions runtime.",
                other
            ))
        }
    }

    let mut transcript_messages = vec![
        visible_text_message("user", &request.prompt),
        hidden_chat_message(json!({
            "role": "user",
            "content": request.prompt,
        })),
    ];
    let resolved_profile = resolve_turn_profile(request);
    runtime_state.ensure_storage(app).await?;
    let mut instructions =
        agent_instructions_for_request(&runtime_state, request, Some(&runtime)).await;
    let requested_tool_choice = tool_choice_for_task(request, &resolved_profile);
    let (_, downgraded_tool_choice) =
        effective_tool_choice_for_provider(&runtime.provider, requested_tool_choice);
    if downgraded_tool_choice {
        instructions.push_str(
            "\n[Tool-calling fallback]\n\
            This provider may ignore tool_choice='required'. You MUST call at least one appropriate tool before finalizing the answer for this turn.\n",
        );
    }
    let mut next_messages = transcript_to_chat_messages(&instructions, request, history);
    compact_chat_messages(&mut next_messages);
    let mut budget = TurnBudget::new(
        max_rounds_for_task(&resolved_profile),
        sampling_profile_params(
            Some(&resolved_profile.sampling_profile),
            Some(&runtime.sampling_profiles),
        )
        .map(|(_, _, max_tokens)| max_tokens),
        None,
    );
    let mut tracker = ToolCallTracker::new(budget.max_rounds);
    for round_idx in 0..budget.max_rounds {
        tracker.current_round = round_idx;
        budget.ensure_round_available(round_idx)?;
        let outcome = stream_chat_completions_response_once(
            None,
            app,
            request,
            &runtime.provider,
            next_messages.clone(),
            None,
        )
        .await?;
        budget.record_output_text(&outcome.assistant_message.content)?;
        let raw_assistant = raw_assistant_message(&outcome.assistant_message);

        transcript_messages.push(visible_assistant_message(
            &outcome.assistant_message.content,
            &outcome.tool_calls,
        ));
        transcript_messages.push(hidden_chat_message(raw_assistant.clone()));

        if outcome.tool_calls.is_empty() {
            return Ok(AgentTurnOutcome {
                response_id: None,
                messages: transcript_messages,
                suspended: false,
            });
        }

        let mut tool_results_messages = vec![raw_assistant];
        let mut invalid_tool_arguments_detected = false;

        // Record calls in tracker before execution
        for call in &outcome.tool_calls {
            tracker.record_call(&call.tool_name, &call.arguments);
        }

        let executed_calls =
            execute_tool_calls(None, &runtime_state, request, outcome.tool_calls, None).await;
        for executed in &executed_calls.executed {
            let result = executed.result.clone();
            if result.content.get("error").and_then(Value::as_str) == Some(AGENT_CANCELLED_MESSAGE)
            {
                return Err(AGENT_CANCELLED_MESSAGE.to_string());
            }
            if tool_result_has_invalid_arguments_error(&result) {
                invalid_tool_arguments_detected = true;
            }
            let feedback = tool_result_feedback_for_model(&result);
            budget.record_output_text(&feedback)?;
            transcript_messages.push(visible_tool_result_message(
                &result.call_id,
                &result.preview,
                result.is_error,
            ));
            tool_results_messages.push(json!({
                "role": "tool",
                "tool_call_id": result.call_id,
                "content": feedback,
            }));
            transcript_messages.push(hidden_chat_message(
                tool_results_messages
                    .last()
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            ));
        }

        if executed_calls.suspended {
            return Ok(AgentTurnOutcome {
                response_id: None,
                messages: transcript_messages,
                suspended: true,
            });
        }

        next_messages.extend(tool_results_messages);
        compact_chat_messages(&mut next_messages);

        // Inject loop-guard warnings / progress checkpoint
        if let Some(injection) = tracker.build_injection(round_idx) {
            next_messages.push(json!({
                "role": "system",
                "content": injection,
            }));
        }

        if invalid_tool_arguments_detected {
            next_messages.push(json!({
                "role": "system",
                "content": TOOL_ARGUMENTS_RETRY_HINT,
            }));
        }
    }

    Err(format!(
        "{} tool loop exceeded {} rounds; aborting to avoid an infinite agent loop.",
        provider_display_name(&runtime.provider),
        max_rounds_for_task(&resolved_profile)
    ))
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
    Err(format!(
        "{} does not support cancel in the current local runtime yet (active provider: {}).",
        provider_display_name(provider),
        config.provider
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        effective_tool_choice_for_provider, merge_stream_fragment, provider_display_name,
        provider_supports_transport, transcript_to_chat_messages,
    };
    use crate::agent::build_agent_instructions;
    use crate::agent::provider::AgentTurnDescriptor;
    use serde_json::json;

    fn make_request(prompt: &str) -> AgentTurnDescriptor {
        AgentTurnDescriptor {
            project_path: "/tmp/project".to_string(),
            prompt: prompt.to_string(),
            tab_id: "tab-test".to_string(),
            model: None,
            local_session_id: None,
            previous_response_id: None,
            turn_profile: None,
        }
    }

    #[test]
    fn converts_text_transcript_into_chat_messages() {
        let history = vec![
            json!({
                "type": "user",
                "message": {
                    "content": [{ "type": "text", "text": "hello" }]
                }
            }),
            json!({
                "type": "assistant",
                "message": {
                    "content": [{ "type": "text", "text": "hi there" }]
                }
            }),
        ];

        let request = make_request("continue");
        let instructions = build_agent_instructions(&request);
        let messages = transcript_to_chat_messages(&instructions, &request, &history);
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["role"], "system");
        assert!(messages[0]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("execution-oriented agent"));
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "hello");
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[2]["content"], "hi there");
        assert_eq!(messages[3]["role"], "user");
        assert_eq!(messages[3]["content"], "continue");
    }

    #[test]
    fn uses_raw_chat_messages_when_present() {
        let history = vec![
            json!({
                "type": "user",
                "message": {
                    "content": [{ "type": "text", "text": "display only" }]
                }
            }),
            json!({
                "type": "chat_message",
                "message": {
                    "role": "user",
                    "content": "real prompt"
                }
            }),
        ];

        let request = make_request("continue");
        let instructions = build_agent_instructions(&request);
        let messages = transcript_to_chat_messages(&instructions, &request, &history);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "system");
        assert!(messages[0]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("execution-oriented agent"));
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "real prompt");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"], "continue");
    }

    #[test]
    fn merge_fragment_handles_cumulative_and_incremental_chunks() {
        assert_eq!(merge_stream_fragment("", "abc"), "abc");
        assert_eq!(merge_stream_fragment("abc", "abcdef"), "def");
        assert_eq!(merge_stream_fragment("abc", "def"), "def");
    }

    #[test]
    fn provider_transport_matrix_includes_deepseek() {
        assert!(provider_supports_transport("minimax"));
        assert!(provider_supports_transport("deepseek"));
        assert!(!provider_supports_transport("openai"));
        assert_eq!(
            provider_display_name("deepseek"),
            "DeepSeek Chat Completions"
        );
    }

    #[test]
    fn deepseek_downgrades_required_tool_choice_to_auto() {
        let (choice, downgraded) = effective_tool_choice_for_provider("deepseek", "required");
        assert_eq!(choice, "auto");
        assert!(downgraded);

        let (choice_minimax, downgraded_minimax) =
            effective_tool_choice_for_provider("minimax", "required");
        assert_eq!(choice_minimax, "required");
        assert!(!downgraded_minimax);
    }

    #[test]
    fn reconstructs_tool_context_from_visible_transcript_when_raw_history_is_missing() {
        let history = vec![
            json!({
                "type": "assistant",
                "message": {
                    "content": [
                        { "type": "text", "text": "I'll patch the file." },
                        {
                            "type": "tool_use",
                            "id": "call_1",
                            "name": "apply_text_patch",
                            "input": {
                                "path": "main.tex",
                                "expected_old_text": "old",
                                "new_text": "new"
                            }
                        }
                    ]
                }
            }),
            json!({
                "type": "user",
                "message": {
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": "call_1",
                            "content": "Edit applied successfully to main.tex.",
                            "is_error": false
                        }
                    ]
                }
            }),
        ];

        let request = make_request("continue");
        let instructions = build_agent_instructions(&request);
        let messages = transcript_to_chat_messages(&instructions, &request, &history);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_1");
    }
}
