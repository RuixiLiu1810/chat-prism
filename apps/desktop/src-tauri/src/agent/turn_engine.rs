use futures::future::join_all;
use serde_json::{json, Value};
use tauri::{Emitter, WebviewWindow};
use tokio::sync::watch;

use super::events::{
    AgentApprovalRequestedEvent, AgentErrorEvent, AgentEventEnvelope, AgentEventPayload,
    AgentMessageDeltaEvent, AgentReviewArtifactReadyEvent, AgentStatusEvent, AgentToolCallEvent,
    AgentToolInterruptEvent, AgentToolInterruptPhase, AgentToolResultEvent, AgentToolResumedEvent,
    AgentTurnResumedEvent, AgentWorkflowCheckpointApprovedEvent,
    AgentWorkflowCheckpointRejectedEvent, AgentWorkflowCheckpointRequestedEvent, AGENT_EVENT_NAME,
};
use super::provider::AgentTurnDescriptor;
use super::resolve_turn_profile;
use super::session::{AgentRuntimeState, PendingTurnResume};
use super::telemetry::{record_tool_execution, ToolExecutionTimer};
use super::tools::{
    check_tool_call_policy, execute_tool_call, is_document_tool_name, is_parallel_safe_tool,
    is_reviewable_edit_tool, summarize_tool_target, tool_result_display_value,
    tool_result_requires_approval, tool_result_review_ready, AgentToolCall, AgentToolResult,
    ToolExecutionPolicyContext,
};
use super::AGENT_CANCELLED_MESSAGE;

#[derive(Debug, Clone)]
pub struct ExecutedToolCall {
    pub result: AgentToolResult,
}

#[derive(Debug, Clone)]
pub struct ExecutedToolBatch {
    pub executed: Vec<ExecutedToolCall>,
    pub suspended: bool,
}

#[derive(Debug, Clone)]
pub struct TurnBudget {
    pub max_rounds: u32,
    pub max_output_tokens: Option<u32>,
    pub consumed_output_tokens: u32,
    pub abort_rx: Option<watch::Receiver<bool>>,
}

fn derive_turn_output_budget(max_rounds: u32, per_call_max_output_tokens: u32) -> u32 {
    // The provider max token value is per request, while one local turn can span
    // multiple tool rounds. Scale the turn budget conservatively so long-running
    // edit/document turns do not fail prematurely on the first few rounds.
    let round_multiplier = max_rounds.clamp(1, 4);
    let scaled = per_call_max_output_tokens.saturating_mul(round_multiplier);
    scaled.clamp(8_192, 32_768)
}

impl TurnBudget {
    pub fn new(
        max_rounds: u32,
        max_output_tokens: Option<u32>,
        abort_rx: Option<watch::Receiver<bool>>,
    ) -> Self {
        Self {
            max_rounds,
            max_output_tokens: max_output_tokens
                .map(|per_call| derive_turn_output_budget(max_rounds, per_call)),
            consumed_output_tokens: 0,
            abort_rx,
        }
    }

    pub fn clone_abort_rx(&self) -> Option<watch::Receiver<bool>> {
        self.abort_rx.clone()
    }

    pub fn ensure_round_available(&self, round_index: u32) -> Result<(), String> {
        self.ensure_not_cancelled()?;
        if round_index >= self.max_rounds {
            return Err(format!(
                "Agent turn exceeded the configured round budget of {}.",
                self.max_rounds
            ));
        }
        Ok(())
    }

    pub fn ensure_not_cancelled(&self) -> Result<(), String> {
        if self
            .abort_rx
            .as_ref()
            .map(|rx| *rx.borrow())
            .unwrap_or(false)
        {
            Err(AGENT_CANCELLED_MESSAGE.to_string())
        } else {
            Ok(())
        }
    }

    pub fn record_output_text(&mut self, text: &str) -> Result<(), String> {
        self.consumed_output_tokens = self
            .consumed_output_tokens
            .saturating_add(estimate_tokens(text));
        if let Some(limit) = self.max_output_tokens {
            if self.consumed_output_tokens > limit {
                return Err(format!(
                    "Agent turn exceeded the configured output budget of {} tokens.",
                    limit
                ));
            }
        }
        Ok(())
    }
}

fn estimate_tokens(text: &str) -> u32 {
    let chars = text.chars().count();
    chars.div_ceil(4) as u32
}

fn request_has_binary_attachment_context(request: &AgentTurnDescriptor) -> bool {
    request.prompt.lines().any(|line| {
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

pub fn should_surface_assistant_text(text: &str, tool_calls: &[AgentToolCall]) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    !tool_calls
        .iter()
        .any(|call| is_reviewable_edit_tool(&call.tool_name))
}

pub fn tool_result_feedback_for_model(result: &AgentToolResult) -> String {
    let approval_required = result
        .content
        .get("approvalRequired")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if result.is_error {
        if approval_required {
            return "The requested edit has been staged for user review and approval. Do not retry this edit unless the user requests a different change.".to_string();
        }

        let error = result
            .content
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Tool execution failed.");

        let correction = if error.contains("not found verbatim")
            || error.contains("expected text was not found")
        {
            "Read the file first and retry with an exact verbatim text match, including whitespace and line breaks."
        } else if error.contains("matched multiple")
            || error.contains("more specific edit tool call")
        {
            "Retry with a longer, more specific exact excerpt that uniquely identifies the target location."
        } else if error.contains("selection-scoped edits must not use write_file") {
            "Use replace_selected_text when a valid selection anchor exists, or read_file followed by apply_text_patch for an exact in-file patch."
        } else if error
            .contains("attachment-backed PDF/DOCX analysis must not use run_shell_command")
            || error.contains(
                "attachment-backed PDF/DOCX analysis must not use read_file on binary resources",
            )
        {
            "Use read_document instead of probing the binary attachment again."
        } else if error.contains("Invalid tool arguments JSON") {
            "Retry with valid JSON arguments and ensure required fields are present."
        } else {
            "Verify the target file and exact input text before retrying."
        };

        return format!("Error: {} {}", error, correction);
    }

    match result.tool_name.as_str() {
        "read_file" => {
            let path = result
                .content
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("file");
            let content = result
                .content
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("");
            let truncated = result
                .content
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if truncated {
                format!(
                    "Read {} successfully. File content (truncated):\n{}",
                    path, content
                )
            } else {
                format!("Read {} successfully. File content:\n{}", path, content)
            }
        }
        "apply_text_patch" | "replace_selected_text" | "write_file" => {
            let path = result
                .content
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("file");
            let written = result
                .content
                .get("written")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if written {
                format!("Edit applied successfully to {}.", path)
            } else {
                format!("Reviewable edit prepared for {}.", path)
            }
        }
        "inspect_resource" => {
            let path = result
                .content
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("resource");
            let kind = result
                .content
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("resource");
            let status = result
                .content
                .get("extractionStatus")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let fallback_used = result
                .content
                .get("fallbackUsed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            format!(
                "Resource inspection for {}: kind={}, extraction_status={}{}.",
                path,
                kind,
                status,
                if fallback_used {
                    ", internal shell fallback available/used"
                } else {
                    ""
                }
            )
        }
        "read_document_excerpt" => {
            let path = result
                .content
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("document");
            let excerpt = result
                .content
                .get("excerpt")
                .and_then(Value::as_str)
                .unwrap_or("");
            let fallback_used = result
                .content
                .get("fallbackUsed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if fallback_used {
                format!(
                    "Document excerpt from {} (using internal controlled fallback extraction):\n{}",
                    path, excerpt
                )
            } else {
                format!("Document excerpt from {}:\n{}", path, excerpt)
            }
        }
        "read_document" => {
            let path = result
                .content
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("document");
            let excerpt = result
                .content
                .get("excerpt")
                .and_then(Value::as_str)
                .unwrap_or("");
            let query = result.content.get("query").and_then(Value::as_str);
            let fallback_used = result
                .content
                .get("fallbackUsed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let matches = result
                .content
                .get("matches")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .take(4)
                        .filter_map(|entry| {
                            let label = entry.get("label").and_then(Value::as_str)?;
                            let snippet = entry.get("snippet").and_then(Value::as_str)?;
                            Some(format!("- {}: {}", label, snippet))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            if let Some(text) = query {
                if matches.is_empty() {
                    format!(
                        "Read document {} but found no relevant evidence for '{}'{}.",
                        path,
                        text,
                        if fallback_used {
                            " after internal fallback extraction"
                        } else {
                            ""
                        }
                    )
                } else {
                    format!(
                        "Relevant document evidence from {} for query '{}'{}:\n{}",
                        path,
                        text,
                        if fallback_used {
                            " (using internal controlled fallback extraction)"
                        } else {
                            ""
                        },
                        matches.join("\n")
                    )
                }
            } else if fallback_used {
                format!(
                    "Document excerpt from {} (using internal controlled fallback extraction):\n{}",
                    path, excerpt
                )
            } else {
                format!("Document excerpt from {}:\n{}", path, excerpt)
            }
        }
        "search_document_text" | "get_document_evidence" => {
            let path = result
                .content
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("document");
            let query = result
                .content
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("query");
            let matches = result
                .content
                .get("matches")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .take(4)
                        .filter_map(|entry| {
                            let label = entry.get("label").and_then(Value::as_str)?;
                            let snippet = entry.get("snippet").and_then(Value::as_str)?;
                            Some(format!("- {}: {}", label, snippet))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let fallback_used = result
                .content
                .get("fallbackUsed")
                .and_then(Value::as_bool)
                .unwrap_or(false);

            if matches.is_empty() {
                format!(
                    "No relevant document matches were found in {} for query '{}'{}.",
                    path,
                    query,
                    if fallback_used {
                        " after internal fallback extraction"
                    } else {
                        ""
                    }
                )
            } else {
                format!(
                    "Relevant document evidence from {} for query '{}'{}:\n{}",
                    path,
                    query,
                    if fallback_used {
                        " (using internal controlled fallback extraction)"
                    } else {
                        ""
                    },
                    matches.join("\n")
                )
            }
        }
        "draft_section" => {
            let section = result
                .content
                .get("sectionType")
                .and_then(Value::as_str)
                .unwrap_or("section");
            let draft = result
                .content
                .get("draft")
                .and_then(Value::as_str)
                .unwrap_or("");
            format!("Drafted {} content:\n{}", section, draft)
        }
        "restructure_outline" => {
            let count = result
                .content
                .get("revisedOutline")
                .and_then(Value::as_array)
                .map(|items| items.len())
                .unwrap_or(0);
            let added = result
                .content
                .get("addedSectionCount")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            format!(
                "Restructured manuscript outline into {} sections ({} added).",
                count, added
            )
        }
        "check_consistency" => {
            let summary = result
                .content
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("Consistency scan completed.");
            let findings = result
                .content
                .get("findings")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .take(3)
                        .filter_map(|entry| entry.get("message").and_then(Value::as_str))
                        .map(|message| format!("- {}", message))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if findings.is_empty() {
                summary.to_string()
            } else {
                format!("{}\n{}", summary, findings.join("\n"))
            }
        }
        "generate_abstract" => {
            let abstract_text = result
                .content
                .get("abstract")
                .and_then(Value::as_str)
                .unwrap_or("");
            format!("Generated abstract:\n{}", abstract_text)
        }
        "insert_citation" => result
            .content
            .get("summary")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| "Citation insertion completed.".to_string()),
        _ => {
            if result.preview.trim().is_empty() {
                "Tool completed successfully.".to_string()
            } else {
                result.preview.clone()
            }
        }
    }
}

pub fn tool_result_status(tool_name: &str, result_content: &Value) -> (&'static str, String) {
    let synthetic = AgentToolResult {
        tool_name: tool_name.to_string(),
        call_id: String::new(),
        is_error: false,
        content: result_content.clone(),
        preview: String::new(),
    };
    let approval_required = tool_result_requires_approval(&synthetic);
    let review_ready = tool_result_review_ready(&synthetic);

    if approval_required && review_ready && is_reviewable_edit_tool(tool_name) {
        return (
            "review_ready",
            "Diff is ready for review before the edit is applied.".to_string(),
        );
    }

    if approval_required {
        return (
            "awaiting_approval",
            format!("{} is waiting for approval.", tool_name),
        );
    }

    (
        "tool_result_ready",
        format!("{} finished. Continuing the task...", tool_name),
    )
}

pub fn emit_status(window: Option<&WebviewWindow>, tab_id: &str, stage: &str, message: &str) {
    if let Some(window) = window {
        if let Err(err) = window.emit(
            AGENT_EVENT_NAME,
            AgentEventEnvelope {
                tab_id: tab_id.to_string(),
                payload: AgentEventPayload::Status(AgentStatusEvent {
                    stage: stage.to_string(),
                    message: message.to_string(),
                }),
            },
        ) {
            eprintln!(
                "[agent][emit] failed to send status event {} for {}: {}",
                stage, tab_id, err
            );
        }
    }
}

pub fn emit_error(window: Option<&WebviewWindow>, tab_id: &str, code: &str, message: String) {
    if let Some(window) = window {
        if let Err(err) = window.emit(
            AGENT_EVENT_NAME,
            AgentEventEnvelope {
                tab_id: tab_id.to_string(),
                payload: AgentEventPayload::Error(AgentErrorEvent {
                    code: code.to_string(),
                    message,
                }),
            },
        ) {
            eprintln!(
                "[agent][emit] failed to send error event {} for {}: {}",
                code, tab_id, err
            );
        }
    }
}

pub fn emit_text_delta(window: Option<&WebviewWindow>, tab_id: &str, delta: &str) {
    if let Some(window) = window {
        if let Err(err) = window.emit(
            AGENT_EVENT_NAME,
            AgentEventEnvelope {
                tab_id: tab_id.to_string(),
                payload: AgentEventPayload::MessageDelta(AgentMessageDeltaEvent {
                    delta: delta.to_string(),
                }),
            },
        ) {
            eprintln!(
                "[agent][emit] failed to send delta event for {}: {}",
                tab_id, err
            );
        }
    }
}

pub fn emit_tool_call(
    window: Option<&WebviewWindow>,
    tab_id: &str,
    tool_name: &str,
    call_id: &str,
    input: Value,
) {
    if let Some(window) = window {
        if let Err(err) = window.emit(
            AGENT_EVENT_NAME,
            AgentEventEnvelope {
                tab_id: tab_id.to_string(),
                payload: AgentEventPayload::ToolCall(AgentToolCallEvent {
                    tool_name: tool_name.to_string(),
                    call_id: call_id.to_string(),
                    input,
                }),
            },
        ) {
            eprintln!(
                "[agent][emit] failed to send tool_call event {} for {}: {}",
                tool_name, tab_id, err
            );
        }
    }
}

pub fn emit_tool_result(
    window: Option<&WebviewWindow>,
    tab_id: &str,
    tool_name: &str,
    call_id: &str,
    is_error: bool,
    preview: String,
    content: Value,
    display: Value,
) {
    if let Some(window) = window {
        if let Err(err) = window.emit(
            AGENT_EVENT_NAME,
            AgentEventEnvelope {
                tab_id: tab_id.to_string(),
                payload: AgentEventPayload::ToolResult(AgentToolResultEvent {
                    tool_name: tool_name.to_string(),
                    call_id: call_id.to_string(),
                    is_error,
                    preview,
                    content,
                    display,
                }),
            },
        ) {
            eprintln!(
                "[agent][emit] failed to send tool_result event {} for {}: {}",
                tool_name, tab_id, err
            );
        }
    }
}

pub fn emit_tool_resumed(
    window: Option<&WebviewWindow>,
    tab_id: &str,
    tool_name: &str,
    target_path: Option<&str>,
    message: &str,
) {
    if let Some(window) = window {
        if let Err(err) = window.emit(
            AGENT_EVENT_NAME,
            AgentEventEnvelope {
                tab_id: tab_id.to_string(),
                payload: AgentEventPayload::ToolResumed(AgentToolResumedEvent {
                    tool_name: tool_name.to_string(),
                    target_path: target_path.map(str::to_string),
                    message: message.to_string(),
                }),
            },
        ) {
            eprintln!(
                "[agent][emit] failed to send tool_resumed event {} for {}: {}",
                tool_name, tab_id, err
            );
        }
        emit_tool_interrupt_state(
            Some(window),
            tab_id,
            AgentToolInterruptPhase::Resumed,
            Some(tool_name),
            None,
            target_path,
            Some(tool_name),
            false,
            false,
            message,
        );
    }
}

pub fn emit_turn_resumed(
    window: Option<&WebviewWindow>,
    tab_id: &str,
    local_session_id: Option<&str>,
    message: &str,
) {
    if let Some(window) = window {
        if let Err(err) = window.emit(
            AGENT_EVENT_NAME,
            AgentEventEnvelope {
                tab_id: tab_id.to_string(),
                payload: AgentEventPayload::TurnResumed(AgentTurnResumedEvent {
                    local_session_id: local_session_id.map(str::to_string),
                    message: message.to_string(),
                }),
            },
        ) {
            eprintln!(
                "[agent][emit] failed to send turn_resumed event for {}: {}",
                tab_id, err
            );
        }
        emit_tool_interrupt_state(
            Some(window),
            tab_id,
            AgentToolInterruptPhase::Cleared,
            None,
            None,
            None,
            None,
            false,
            false,
            message,
        );
    }
}

pub fn emit_workflow_checkpoint_requested(
    window: Option<&WebviewWindow>,
    tab_id: &str,
    workflow_type: &str,
    stage: &str,
    message: &str,
) {
    let Some(window) = window else {
        return;
    };
    if let Err(err) = window.emit(
        AGENT_EVENT_NAME,
        AgentEventEnvelope {
            tab_id: tab_id.to_string(),
            payload: AgentEventPayload::WorkflowCheckpointRequested(
                AgentWorkflowCheckpointRequestedEvent {
                    workflow_type: workflow_type.to_string(),
                    stage: stage.to_string(),
                    message: message.to_string(),
                },
            ),
        },
    ) {
        eprintln!(
            "[agent][emit] failed to send workflow_checkpoint_requested for {}: {}",
            tab_id, err
        );
    }
}

pub fn emit_workflow_checkpoint_approved(
    window: Option<&WebviewWindow>,
    tab_id: &str,
    workflow_type: &str,
    from_stage: &str,
    to_stage: &str,
    completed: bool,
    message: &str,
) {
    let Some(window) = window else {
        return;
    };
    if let Err(err) = window.emit(
        AGENT_EVENT_NAME,
        AgentEventEnvelope {
            tab_id: tab_id.to_string(),
            payload: AgentEventPayload::WorkflowCheckpointApproved(
                AgentWorkflowCheckpointApprovedEvent {
                    workflow_type: workflow_type.to_string(),
                    from_stage: from_stage.to_string(),
                    to_stage: to_stage.to_string(),
                    completed,
                    message: message.to_string(),
                },
            ),
        },
    ) {
        eprintln!(
            "[agent][emit] failed to send workflow_checkpoint_approved for {}: {}",
            tab_id, err
        );
    }
}

pub fn emit_workflow_checkpoint_rejected(
    window: Option<&WebviewWindow>,
    tab_id: &str,
    workflow_type: &str,
    stage: &str,
    message: &str,
) {
    let Some(window) = window else {
        return;
    };
    if let Err(err) = window.emit(
        AGENT_EVENT_NAME,
        AgentEventEnvelope {
            tab_id: tab_id.to_string(),
            payload: AgentEventPayload::WorkflowCheckpointRejected(
                AgentWorkflowCheckpointRejectedEvent {
                    workflow_type: workflow_type.to_string(),
                    stage: stage.to_string(),
                    message: message.to_string(),
                },
            ),
        },
    ) {
        eprintln!(
            "[agent][emit] failed to send workflow_checkpoint_rejected for {}: {}",
            tab_id, err
        );
    }
}

fn emit_tool_interrupt_state(
    window: Option<&WebviewWindow>,
    tab_id: &str,
    phase: AgentToolInterruptPhase,
    tool_name: Option<&str>,
    call_id: Option<&str>,
    target_path: Option<&str>,
    approval_tool_name: Option<&str>,
    review_ready: bool,
    can_resume: bool,
    message: &str,
) {
    let Some(window) = window else {
        return;
    };

    if let Err(err) = window.emit(
        AGENT_EVENT_NAME,
        AgentEventEnvelope {
            tab_id: tab_id.to_string(),
            payload: AgentEventPayload::ToolInterrupt(AgentToolInterruptEvent {
                phase,
                tool_name: tool_name.map(str::to_string),
                call_id: call_id.map(str::to_string),
                target_path: target_path.map(str::to_string),
                approval_tool_name: approval_tool_name.map(str::to_string),
                review_ready,
                can_resume,
                message: message.to_string(),
            }),
        },
    ) {
        eprintln!(
            "[agent][emit] failed to send tool_interrupt event for {}: {}",
            tab_id, err
        );
    }
}

fn emit_approval_requested(
    window: Option<&WebviewWindow>,
    tab_id: &str,
    tool_name: &str,
    call_id: &str,
    content: &Value,
) {
    let Some(window) = window else {
        return;
    };
    let approval_required = content
        .get("approvalRequired")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !approval_required {
        return;
    }

    let target_path = content
        .get("path")
        .and_then(Value::as_str)
        .map(str::to_string);
    let review_ready = content
        .get("reviewArtifact")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let message = content
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("Tool approval is required.")
        .to_string();

    if let Err(err) = window.emit(
        AGENT_EVENT_NAME,
        AgentEventEnvelope {
            tab_id: tab_id.to_string(),
            payload: AgentEventPayload::ApprovalRequested(AgentApprovalRequestedEvent {
                tool_name: tool_name.to_string(),
                call_id: call_id.to_string(),
                target_path,
                review_ready,
                message,
            }),
        },
    ) {
        eprintln!(
            "[agent][emit] failed to send approval_requested event {} for {}: {}",
            tool_name, tab_id, err
        );
    }
}

fn emit_review_artifact_ready(
    window: Option<&WebviewWindow>,
    tab_id: &str,
    tool_name: &str,
    call_id: &str,
    content: &Value,
) {
    let Some(window) = window else {
        return;
    };

    let Some(path) = content.get("path").and_then(Value::as_str) else {
        return;
    };
    let review_ready = content
        .get("reviewArtifact")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !review_ready {
        return;
    }

    let summary = content
        .get("reviewArtifactPayload")
        .and_then(Value::as_object)
        .and_then(|payload| payload.get("summary"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            content
                .get("summary")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let written = content
        .get("written")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if let Err(err) = window.emit(
        AGENT_EVENT_NAME,
        AgentEventEnvelope {
            tab_id: tab_id.to_string(),
            payload: AgentEventPayload::ReviewArtifactReady(AgentReviewArtifactReadyEvent {
                tool_name: tool_name.to_string(),
                call_id: call_id.to_string(),
                target_path: path.to_string(),
                summary,
                written,
            }),
        },
    ) {
        eprintln!(
            "[agent][emit] failed to send review_artifact_ready event {} for {}: {}",
            tool_name, tab_id, err
        );
    }
}

#[derive(Debug, Clone)]
struct PreparedToolCall {
    call: AgentToolCall,
    target: Option<String>,
    timer: ToolExecutionTimer,
}

async fn prepare_tool_call(
    window: Option<&WebviewWindow>,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    call: AgentToolCall,
) -> PreparedToolCall {
    let input = serde_json::from_str::<Value>(&call.arguments).unwrap_or_else(|_| json!({}));
    let target = summarize_tool_target(&input);
    runtime_state
        .record_tool_running(
            &request.tab_id,
            request.local_session_id.as_deref(),
            &call.tool_name,
            target.as_deref(),
        )
        .await;
    if is_document_tool_name(&call.tool_name) {
        let target_label = target
            .as_deref()
            .map(|value| format!(" for {}", value))
            .unwrap_or_default();
        emit_status(
            window,
            &request.tab_id,
            "document_read_started",
            &format!("Reading document{}...", target_label),
        );
    } else {
        emit_status(
            window,
            &request.tab_id,
            "tool_running",
            &format!("Running {}...", call.tool_name),
        );
    }
    emit_tool_call(
        window,
        &request.tab_id,
        &call.tool_name,
        &call.call_id,
        input.clone(),
    );

    PreparedToolCall {
        call,
        target,
        timer: ToolExecutionTimer::start(),
    }
}

async fn execute_prepared_tool_call(
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    resolved_profile: &super::provider::AgentTurnProfile,
    prepared: PreparedToolCall,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> (PreparedToolCall, AgentToolResult) {
    let policy_context = ToolExecutionPolicyContext {
        task_kind: resolved_profile.task_kind.clone(),
        has_binary_attachment_context: request_has_binary_attachment_context(request),
    };
    let result = if let Some(blocked) =
        check_tool_call_policy(policy_context, &prepared.call, prepared.target.as_deref())
    {
        blocked
    } else {
        execute_tool_call(
            runtime_state,
            &request.tab_id,
            &request.project_path,
            prepared.call.clone(),
            cancel_rx,
        )
        .await
    };

    (prepared, result)
}

async fn handle_tool_result(
    window: Option<&WebviewWindow>,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    prepared: PreparedToolCall,
    result: AgentToolResult,
) -> ExecutedToolCall {
    let result_target = summarize_tool_target(&result.content).or(prepared.target.clone());
    let display = tool_result_display_value(&result);
    runtime_state
        .record_tool_result(
            &request.tab_id,
            request.local_session_id.as_deref(),
            &result.tool_name,
            result_target.as_deref(),
            result.is_error,
        )
        .await;

    emit_tool_result(
        window,
        &request.tab_id,
        &result.tool_name,
        &result.call_id,
        result.is_error,
        result.preview.clone(),
        result.content.clone(),
        display,
    );
    let approval_required = tool_result_requires_approval(&result);
    let review_ready = tool_result_review_ready(&result);
    emit_review_artifact_ready(
        window,
        &request.tab_id,
        &result.tool_name,
        &result.call_id,
        &result.content,
    );
    emit_approval_requested(
        window,
        &request.tab_id,
        &result.tool_name,
        &result.call_id,
        &result.content,
    );
    if approval_required {
        let approval_tool_name = result
            .content
            .get("approvalToolName")
            .and_then(Value::as_str)
            .unwrap_or(&result.tool_name);
        let interrupt_message = result
            .content
            .get("reason")
            .and_then(Value::as_str)
            .or_else(|| result.content.get("summary").and_then(Value::as_str))
            .unwrap_or("Tool approval is required.");
        emit_tool_interrupt_state(
            window,
            &request.tab_id,
            if review_ready {
                AgentToolInterruptPhase::ReviewReady
            } else {
                AgentToolInterruptPhase::AwaitingApproval
            },
            Some(&result.tool_name),
            Some(&result.call_id),
            result_target.as_deref(),
            Some(approval_tool_name),
            review_ready,
            !review_ready,
            interrupt_message,
        );
    }
    if approval_required {
        runtime_state
            .mark_pending_state(
                &request.tab_id,
                request.local_session_id.as_deref(),
                if review_ready { "review" } else { "approval" },
                &result.tool_name,
                result_target.as_deref(),
            )
            .await;
        let approval_tool_name = result
            .content
            .get("approvalToolName")
            .and_then(Value::as_str)
            .unwrap_or(&result.tool_name);
        let action_label = if is_reviewable_edit_tool(approval_tool_name) {
            format!(
                "apply the pending edit{}",
                result_target
                    .as_deref()
                    .map(|target| format!(" for {}", target))
                    .unwrap_or_default()
            )
        } else if approval_tool_name == "run_shell_command" {
            format!(
                "run the pending command{}",
                result_target
                    .as_deref()
                    .map(|target| format!(" on {}", target))
                    .unwrap_or_default()
            )
        } else {
            format!("continue the pending {} action", approval_tool_name)
        };
        let continuation_prompt = format!(
            "A required approval for {} has now been granted. Resume the suspended task in the current session context. Continue from the blocked tool stage instead of restarting from scratch. Use the minimal next tool action needed.",
            action_label
        );
        runtime_state
            .store_pending_turn(PendingTurnResume {
                project_path: request.project_path.clone(),
                tab_id: request.tab_id.clone(),
                local_session_id: request.local_session_id.clone(),
                model: request.model.clone(),
                turn_profile: request.turn_profile.clone(),
                approval_tool_name: approval_tool_name.to_string(),
                target_label: result_target.clone(),
                continuation_prompt,
                created_at: String::new(),
                expires_at: String::new(),
            })
            .await;
    } else if is_reviewable_edit_tool(&result.tool_name) || result.tool_name == "run_shell_command"
    {
        runtime_state
            .clear_pending_turn(&request.tab_id, request.local_session_id.as_deref())
            .await;
    }

    if is_document_tool_name(&result.tool_name) {
        let target_label = result_target
            .as_deref()
            .map(|value| format!(" for {}", value))
            .unwrap_or_default();
        if result.is_error {
            let reason = result
                .content
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("document read failed");
            emit_status(
                window,
                &request.tab_id,
                "document_read_failed",
                &format!("Document read failed{}: {}", target_label, reason),
            );
        } else {
            emit_status(
                window,
                &request.tab_id,
                "document_read_ready",
                &format!("Document read ready{}.", target_label),
            );
        }
    } else {
        let (stage, message) = tool_result_status(&result.tool_name, &result.content);
        emit_status(window, &request.tab_id, stage, &message);
    }
    record_tool_execution(
        runtime_state,
        request,
        &result,
        result_target,
        prepared.timer.elapsed(),
    )
    .await;

    ExecutedToolCall { result }
}

pub async fn execute_tool_calls(
    window: Option<&WebviewWindow>,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    tool_calls: Vec<AgentToolCall>,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> ExecutedToolBatch {
    let mut executed = Vec::with_capacity(tool_calls.len());
    let resolved_profile = resolve_turn_profile(request);
    let mut index = 0usize;
    let mut suspended = false;

    while index < tool_calls.len() {
        let parallel_batch = is_parallel_safe_tool(&tool_calls[index].tool_name);
        let mut batch = vec![tool_calls[index].clone()];
        index += 1;

        while parallel_batch
            && index < tool_calls.len()
            && is_parallel_safe_tool(&tool_calls[index].tool_name)
        {
            batch.push(tool_calls[index].clone());
            index += 1;
        }

        let mut prepared_calls = Vec::with_capacity(batch.len());
        for call in batch {
            prepared_calls.push(prepare_tool_call(window, runtime_state, request, call).await);
        }

        let batch_results = if parallel_batch && prepared_calls.len() > 1 {
            join_all(prepared_calls.into_iter().map(|prepared| {
                execute_prepared_tool_call(
                    runtime_state,
                    request,
                    &resolved_profile,
                    prepared,
                    cancel_rx.clone(),
                )
            }))
            .await
        } else {
            let mut results = Vec::new();
            for prepared in prepared_calls {
                results.push(
                    execute_prepared_tool_call(
                        runtime_state,
                        request,
                        &resolved_profile,
                        prepared,
                        cancel_rx.clone(),
                    )
                    .await,
                );
            }
            results
        };

        for (prepared, result) in batch_results {
            let approval_required = tool_result_requires_approval(&result);
            executed
                .push(handle_tool_result(window, runtime_state, request, prepared, result).await);
            if approval_required {
                suspended = true;
                break;
            }
        }

        if suspended {
            break;
        }
    }

    ExecutedToolBatch {
        executed,
        suspended,
    }
}
