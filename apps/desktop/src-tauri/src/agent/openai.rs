use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::Instant;
use tauri::{Manager, WebviewWindow};
use tokio::sync::watch;

use crate::settings;

use super::provider::{
    AgentProvider, AgentSamplingProfile, AgentStatus, AgentTurnDescriptor, AgentTurnHandle,
};
use super::session::AgentRuntimeState;
use super::telemetry::{
    document_artifact_miss, document_fallback_used, record_document_question_metrics,
};
use super::tools::{
    default_tool_specs, is_document_tool_name, to_openai_tool_schema, AgentToolCall,
};
use super::turn_engine::{
    emit_error, emit_status, emit_text_delta, execute_tool_calls, should_surface_assistant_text,
    tool_result_feedback_for_model, TurnBudget,
};
use super::{
    agent_instructions_for_request, max_rounds_for_task, resolve_turn_profile, tool_choice_for_task,
};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const AGENT_CANCELLED_MESSAGE: &str = "Agent run cancelled by user.";

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

#[derive(Debug, Default, Clone)]
pub struct OpenAiProvider;

#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
    pub source: String,
    pub sampling_profiles: Option<settings::AgentSamplingProfilesConfig>,
}

impl OpenAiProvider {
    #[allow(dead_code)]
    fn skeleton_error() -> String {
        "OpenAI agent provider skeleton is in place, but the Responses API transport is not wired yet."
            .to_string()
    }

    fn from_env() -> Result<OpenAiConfig, String> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| "OPENAI_API_KEY is not set".to_string())?
            .trim()
            .to_string();

        if api_key.is_empty() {
            return Err("OPENAI_API_KEY is empty".to_string());
        }

        let base_url = std::env::var("OPENAI_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        let default_model = std::env::var("OPENAI_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "gpt-5.4".to_string());

        Ok(OpenAiConfig {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            default_model,
            source: "env".to_string(),
            sampling_profiles: None,
        })
    }

    fn from_runtime(
        app: &tauri::AppHandle,
        project_root: Option<&str>,
    ) -> Result<OpenAiConfig, String> {
        let runtime = settings::load_agent_runtime(app, project_root)?;
        if runtime.provider != "openai" {
            return Err(format!(
                "OpenAI Responses adapter cannot handle provider `{}`.",
                runtime.provider
            ));
        }

        if let Some(api_key) = runtime.api_key.filter(|value| !value.trim().is_empty()) {
            return Ok(OpenAiConfig {
                api_key,
                base_url: runtime.base_url.trim_end_matches('/').to_string(),
                default_model: runtime.model,
                source: "settings".to_string(),
                sampling_profiles: Some(runtime.sampling_profiles),
            });
        }

        Self::from_env()
    }
}

#[derive(Debug, Clone)]
struct StreamResponseOutcome {
    response_id: Option<String>,
    tool_calls: Vec<AgentToolCall>,
    assistant_text: String,
}

#[derive(Debug, Clone)]
pub struct AgentTurnOutcome {
    pub response_id: Option<String>,
    pub messages: Vec<Value>,
    pub suspended: bool,
}

impl AgentProvider for OpenAiProvider {
    fn provider_id(&self) -> &'static str {
        "openai"
    }

    fn display_name(&self) -> &'static str {
        "OpenAI Responses"
    }

    fn default_model(&self) -> Option<&'static str> {
        Some("gpt-5.4")
    }

    fn check_status(&self) -> AgentStatus {
        match Self::from_env() {
            Ok(config) => AgentStatus {
                provider: self.provider_id().to_string(),
                display_name: self.display_name().to_string(),
                ready: true,
                mode: "text_streaming_ready".to_string(),
                message: "OpenAI Responses text streaming is configured via environment variables."
                    .to_string(),
                default_model: Some(config.default_model),
            },
            Err(message) => AgentStatus {
                provider: self.provider_id().to_string(),
                display_name: self.display_name().to_string(),
                ready: false,
                mode: "env_missing".to_string(),
                message,
                default_model: self.default_model().map(str::to_string),
            },
        }
    }

    fn start_turn(&self, _request: &AgentTurnDescriptor) -> Result<AgentTurnHandle, String> {
        Err(Self::skeleton_error())
    }

    fn continue_turn(&self, _request: &AgentTurnDescriptor) -> Result<AgentTurnHandle, String> {
        Err(Self::skeleton_error())
    }

    fn cancel_turn(&self, _response_id: &str) -> Result<(), String> {
        Err(Self::skeleton_error())
    }
}

pub fn runtime_status(app: &tauri::AppHandle) -> AgentStatus {
    match OpenAiProvider::from_runtime(app, None) {
        Ok(config) => AgentStatus {
            provider: "openai".to_string(),
            display_name: "OpenAI Responses".to_string(),
            ready: true,
            mode: if config.source == "settings" {
                "settings_ready".to_string()
            } else {
                "env_fallback_ready".to_string()
            },
            message: if config.source == "settings" {
                "OpenAI Responses is configured through Settings.".to_string()
            } else {
                "OpenAI Responses is configured through environment variables.".to_string()
            },
            default_model: Some(config.default_model),
        },
        Err(message) => AgentStatus {
            provider: "openai".to_string(),
            display_name: "OpenAI Responses".to_string(),
            ready: false,
            mode: "not_configured".to_string(),
            message,
            default_model: Some("gpt-5.4".to_string()),
        },
    }
}

fn extract_response_id(payload: &Value) -> Option<String> {
    payload
        .pointer("/response/id")
        .and_then(Value::as_str)
        .or_else(|| payload.get("response_id").and_then(Value::as_str))
        .or_else(|| payload.get("id").and_then(Value::as_str))
        .map(str::to_string)
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

fn extract_function_call_item(item: &Value) -> Option<AgentToolCall> {
    if item.get("type").and_then(Value::as_str) != Some("function_call") {
        return None;
    }

    let tool_name = item
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)?;
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .or_else(|| item.get("id").and_then(Value::as_str))
        .map(str::to_string)?;
    let arguments = item
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}")
        .to_string();

    Some(AgentToolCall {
        tool_name,
        call_id,
        arguments,
    })
}

async fn stream_response_once(
    window: &WebviewWindow,
    config: &OpenAiConfig,
    request: &AgentTurnDescriptor,
    instructions: &str,
    input: Value,
    previous_response_id: Option<String>,
    mut cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<StreamResponseOutcome, String> {
    let model = request
        .model
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| config.default_model.clone());
    let resolved_profile = resolve_turn_profile(request);

    let mut body = json!({
        "model": model,
        "input": input,
        "instructions": instructions,
        "stream": true,
        "parallel_tool_calls": false,
        "tool_choice": tool_choice_for_task(request, &resolved_profile),
        "tools": default_tool_specs()
            .iter()
            .map(to_openai_tool_schema)
            .collect::<Vec<_>>(),
    });

    if let Some((temperature, top_p, max_output_tokens)) = sampling_profile_params(
        Some(&resolved_profile.sampling_profile),
        config.sampling_profiles.as_ref(),
    ) {
        body["temperature"] = json!(temperature);
        body["top_p"] = json!(top_p);
        body["max_output_tokens"] = json!(max_output_tokens);
    }

    if let Some(previous_response_id) = previous_response_id
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        body["previous_response_id"] = json!(previous_response_id);
    }

    let client = Client::new();
    let url = format!("{}/responses", config.base_url);
    let response = client
        .post(url)
        .bearer_auth(&config.api_key)
        .header("Accept", "text/event-stream")
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|err| format!("OpenAI request failed: {}", err))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let preview = if body.len() > 400 {
            format!("{}...", &body[..400])
        } else {
            body
        };
        return Err(format!(
            "OpenAI Responses request failed with status {}: {}",
            status, preview
        ));
    }

    emit_status(
        Some(window),
        &request.tab_id,
        "streaming",
        "Connected to OpenAI Responses API.",
    );

    let mut response = response;
    let mut buffer = String::new();
    let mut final_response_id = None;
    let mut tool_calls = Vec::new();
    let mut assistant_text = String::new();

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
                chunk = response.chunk() => chunk.map_err(|err| format!("OpenAI streaming read failed: {}", err))?
            }
        } else {
            response
                .chunk()
                .await
                .map_err(|err| format!("OpenAI streaming read failed: {}", err))?
        };

        let Some(chunk) = next_chunk else {
            break;
        };
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some((event_name, data)) = take_next_sse_frame(&mut buffer) {
            if data == "[DONE]" {
                continue;
            }

            let parsed: Value = match serde_json::from_str(&data) {
                Ok(value) => value,
                Err(err) => {
                    emit_error(
                        Some(window),
                        &request.tab_id,
                        "agent_stream_parse_error",
                        format!("Failed to parse streaming event {}: {}", event_name, err),
                    );
                    continue;
                }
            };

            if final_response_id.is_none() {
                final_response_id = extract_response_id(&parsed);
            }

            match event_name.as_str() {
                "response.created" => {
                    emit_status(
                        Some(window),
                        &request.tab_id,
                        "created",
                        "OpenAI response created.",
                    );
                }
                "response.output_text.delta" => {
                    if let Some(delta) = parsed.get("delta").and_then(Value::as_str) {
                        assistant_text.push_str(delta);
                        emit_text_delta(Some(window), &request.tab_id, delta);
                    }
                }
                "response.completed" => {
                    final_response_id = extract_response_id(&parsed).or(final_response_id);
                    if let Some(output_items) =
                        parsed.pointer("/response/output").and_then(Value::as_array)
                    {
                        for item in output_items {
                            if let Some(call) = extract_function_call_item(item) {
                                tool_calls.push(call);
                            }
                        }
                    }
                    emit_status(
                        Some(window),
                        &request.tab_id,
                        "completed",
                        "OpenAI response completed.",
                    );
                }
                "response.failed" | "response.incomplete" | "error" => {
                    let message = parsed
                        .get("message")
                        .and_then(Value::as_str)
                        .or_else(|| {
                            parsed
                                .get("error")
                                .and_then(|error| error.get("message"))
                                .and_then(Value::as_str)
                        })
                        .unwrap_or("OpenAI returned an error event.");
                    emit_error(
                        Some(window),
                        &request.tab_id,
                        "agent_provider_error",
                        message.to_string(),
                    );
                }
                "response.function_call_arguments.done" => {
                    if let Some(item) = parsed.get("item").and_then(extract_function_call_item) {
                        tool_calls.push(item);
                    }
                }
                _ => {}
            }
        }
    }

    Ok(StreamResponseOutcome {
        response_id: final_response_id,
        tool_calls,
        assistant_text,
    })
}

fn make_text_message(role: &str, text: &str) -> Value {
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

fn make_tool_result_message(call_id: &str, preview: &str, is_error: bool) -> Value {
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

pub async fn run_turn_loop(
    window: &WebviewWindow,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
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
    let config = OpenAiProvider::from_runtime(&window.app_handle(), Some(&request.project_path))?;
    let mut previous_response_id = request.previous_response_id.clone();
    let mut next_input = json!([
        {
            "role": "user",
            "content": [
                {
                    "type": "input_text",
                    "text": request.prompt
                }
            ]
        }
    ]);
    let mut latest_response_id = None;
    let mut transcript_messages = vec![make_text_message("user", &request.prompt)];

    let resolved_profile = resolve_turn_profile(request);
    let runtime_settings =
        settings::load_agent_runtime(&window.app_handle(), Some(&request.project_path))?;
    let instructions =
        agent_instructions_for_request(runtime_state, request, Some(&runtime_settings)).await;
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
            config.sampling_profiles.as_ref(),
        )
        .map(|(_, _, max_output_tokens)| max_output_tokens),
        cancel_rx.clone(),
    );
    for round_idx in 0..budget.max_rounds {
        budget.ensure_round_available(round_idx)?;
        let outcome = stream_response_once(
            window,
            &config,
            request,
            &instructions,
            next_input,
            previous_response_id.clone(),
            budget.clone_abort_rx(),
        )
        .await?;
        budget.record_output_text(&outcome.assistant_text)?;

        latest_response_id = outcome.response_id.clone().or(latest_response_id);

        if should_surface_assistant_text(&outcome.assistant_text, &outcome.tool_calls) {
            transcript_messages.push(make_text_message("assistant", &outcome.assistant_text));
        }

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
                response_id: latest_response_id,
                messages: transcript_messages,
                suspended: false,
            });
        }

        let mut seen_call_ids = HashSet::new();
        let deduped_tool_calls: Vec<AgentToolCall> = outcome
            .tool_calls
            .into_iter()
            .filter(|call| seen_call_ids.insert(call.call_id.clone()))
            .collect();
        let round_doc_calls = deduped_tool_calls
            .iter()
            .filter(|call| is_document_tool_name(&call.tool_name))
            .count() as u32;
        if round_doc_calls > 0 {
            doc_tool_rounds = doc_tool_rounds.saturating_add(1);
            doc_tool_calls = doc_tool_calls.saturating_add(round_doc_calls);
        }
        let executed_calls = execute_tool_calls(
            Some(window),
            runtime_state,
            request,
            deduped_tool_calls,
            budget.clone_abort_rx(),
        )
        .await;

        let mut tool_outputs = Vec::new();
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
            let feedback = tool_result_feedback_for_model(&result);
            budget.record_output_text(&feedback)?;
            transcript_messages.push(make_tool_result_message(
                &result.call_id,
                &result.preview,
                result.is_error,
            ));

            tool_outputs.push(json!({
                "type": "function_call_output",
                "call_id": result.call_id,
                "output": feedback,
            }));
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
                response_id: latest_response_id,
                messages: transcript_messages,
                suspended: true,
            });
        }

        previous_response_id = outcome.response_id.or(previous_response_id);
        next_input = Value::Array(tool_outputs);
        emit_status(
            Some(window),
            &request.tab_id,
            "responding_after_tools",
            "Tool results sent back to the model. Continuing...",
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
        "Tool loop exceeded {} rounds; aborting to avoid an infinite agent loop.",
        max_rounds_for_task(&resolved_profile)
    ))
}

pub async fn cancel_response(app: &tauri::AppHandle, response_id: &str) -> Result<(), String> {
    let config = OpenAiProvider::from_runtime(app, None)?;
    let client = Client::new();
    let url = format!("{}/responses/{}/cancel", config.base_url, response_id);
    let response = client
        .post(url)
        .bearer_auth(config.api_key)
        .send()
        .await
        .map_err(|err| format!("OpenAI cancel request failed: {}", err))?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(format!(
            "OpenAI cancel request failed with status {}: {}",
            status, body
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extract_function_call_item, extract_response_id, parse_sse_frame,
        should_surface_assistant_text,
    };
    use crate::agent::tools::AgentToolCall;
    use serde_json::json;

    #[test]
    fn parses_sse_frame_with_event_and_data() {
        let parsed =
            parse_sse_frame("event: response.output_text.delta\ndata: {\"delta\":\"hi\"}\n");
        assert_eq!(
            parsed,
            Some((
                "response.output_text.delta".to_string(),
                "{\"delta\":\"hi\"}".to_string()
            ))
        );
    }

    #[test]
    fn extracts_response_id_from_multiple_shapes() {
        assert_eq!(
            extract_response_id(&json!({ "response": { "id": "resp_1" } })),
            Some("resp_1".to_string())
        );
        assert_eq!(
            extract_response_id(&json!({ "response_id": "resp_2" })),
            Some("resp_2".to_string())
        );
        assert_eq!(
            extract_response_id(&json!({ "id": "resp_3" })),
            Some("resp_3".to_string())
        );
    }

    #[test]
    fn extracts_function_call_item_from_stream_payload() {
        let item = json!({
            "type": "function_call",
            "name": "read_file",
            "call_id": "call_123",
            "arguments": "{\"path\":\"src/main.ts\"}"
        });
        let call = extract_function_call_item(&item).expect("function call should parse");
        assert_eq!(call.tool_name, "read_file");
        assert_eq!(call.call_id, "call_123");
        assert_eq!(call.arguments, "{\"path\":\"src/main.ts\"}");
    }

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
}
