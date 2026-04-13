// Re-export pure types and functions from agent-core.
pub use agent_core::turn_engine::{
    compact_chat_messages, estimate_messages_tokens, estimate_tokens,
    request_has_binary_attachment_context, should_surface_assistant_text,
    tool_result_feedback_for_model, tool_result_has_invalid_arguments_error, tool_result_status,
    ExecutedToolBatch, ExecutedToolCall, ToolCallTracker, TurnBudget,
};

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

// ─── WebviewWindow-based Event Emission (Tauri adapter) ─────────────
// These keep the original Option<&WebviewWindow> signatures. Callers across
// the Tauri codebase will be migrated to use the agent-core EventSink
// abstraction in a future phase.

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
    runtime_state
        .record_collected_references_from_tool_result(
            &request.tab_id,
            request.local_session_id.as_deref(),
            &result.tool_name,
            &result.content,
        )
        .await;
    runtime_state
        .record_academic_artifacts_from_tool_result(
            &request.tab_id,
            request.local_session_id.as_deref(),
            &result.tool_name,
            &result.content,
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
            true,
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

#[cfg(test)]
mod compaction_tests {
    use super::*;
    use serde_json::json;

    fn make_system(text: &str) -> Value {
        json!({"role": "system", "content": text})
    }
    fn make_user(text: &str) -> Value {
        json!({"role": "user", "content": text})
    }
    fn make_assistant_with_tool(text: &str, tool_name: &str) -> Value {
        json!({
            "role": "assistant",
            "content": text,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": tool_name, "arguments": "{}"}
            }]
        })
    }
    fn make_tool_result(call_id: &str, content: &str) -> Value {
        json!({"role": "tool", "tool_call_id": call_id, "content": content})
    }

    #[test]
    fn no_compaction_when_under_limit() {
        let mut messages = vec![
            make_system("system prompt"),
            make_user("hello"),
        ];
        let before_len = messages.len();
        compact_chat_messages(&mut messages);
        assert_eq!(messages.len(), before_len);
    }

    #[test]
    fn compaction_preserves_system_and_recent() {
        // Build a large message array that exceeds 60k tokens.
        // Each filler message ~250 tokens (1000 ASCII chars ÷ 4).
        let filler = "x".repeat(1000);
        let mut messages = vec![make_system("system")];
        // 300 messages × 250 tokens ≈ 75,000 tokens → should trigger compaction.
        for i in 0..300 {
            if i % 3 == 0 {
                messages.push(make_assistant_with_tool(&filler, "read_file"));
            } else if i % 3 == 1 {
                messages.push(make_tool_result("call_1", &filler));
            } else {
                messages.push(make_user(&filler));
            }
        }
        let original_len = messages.len();
        compact_chat_messages(&mut messages);

        // Should have been compacted.
        assert!(messages.len() < original_len);
        // System message intact.
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "system");
        // Second message should be the compaction summary.
        assert_eq!(messages[1]["role"], "system");
        let summary = messages[1]["content"].as_str().unwrap();
        assert!(summary.contains("Context compacted"));
        assert!(summary.contains("read_file"));
    }

    #[test]
    fn compaction_keeps_tool_call_groups_intact() {
        // Create messages just over the limit with clear tool-call groups.
        let filler = "y".repeat(1000);
        let mut messages = vec![make_system("sys")];
        // 80 groups × ~1031 tokens/group ≈ 82k tokens → should trigger compaction.
        for _ in 0..80 {
            messages.push(make_assistant_with_tool(&filler, "run_shell_command"));
            messages.push(make_tool_result("call_1", &filler));
            messages.push(make_tool_result("call_1", &filler));
            messages.push(make_user("ok"));
            messages.push(make_user(&filler));
        }
        compact_chat_messages(&mut messages);

        // After compaction, no "tool" message should appear right after the
        // compaction summary (that would mean we split a group).
        if messages.len() > 2 {
            let after_summary = &messages[2];
            let role = after_summary["role"].as_str().unwrap_or("");
            assert_ne!(role, "tool", "tool message should not follow compaction summary");
        }
    }

    #[test]
    fn estimate_tokens_cjk_vs_ascii() {
        let ascii = "hello world"; // 11 chars → ceil(11/4) = 3
        assert_eq!(estimate_tokens(ascii), 3);

        let cjk = "你好世界"; // 4 CJK chars → (4*3 + 0) / 4 = 3
        assert_eq!(estimate_tokens(cjk), 3);

        let mixed = "hello你好"; // 5 ascii + 2 CJK → (2*3 + 5) / 4 = ceil(11/4) = 3
        assert_eq!(estimate_tokens(mixed), 3);
    }

    #[test]
    fn estimate_messages_tokens_sums_correctly() {
        let messages = vec![
            make_system("hello"),
            make_user("world"),
        ];
        let total = estimate_messages_tokens(&messages);
        // Each: 4 overhead + estimate_tokens(5 chars) = 4 + 2 = 6
        assert_eq!(total, 12);
    }
}
