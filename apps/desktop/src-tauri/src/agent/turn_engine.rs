// Re-export pure types and functions from agent-core.
pub use agent_core::turn_engine::{
    compact_chat_messages, estimate_messages_tokens, estimate_tokens,
    request_has_binary_attachment_context, should_surface_assistant_text,
    tool_result_feedback_for_model, tool_result_has_invalid_arguments_error, tool_result_status,
    ExecutedToolBatch, ExecutedToolCall, ToolCallTracker, ToolExecutorFn, TurnBudget,
};

// Re-export emit functions from agent-core (EventSink-based).
use agent_core::EventSink;
pub use agent_core::{
    emit_agent_complete, emit_approval_requested, emit_error, emit_review_artifact_ready,
    emit_status, emit_text_delta, emit_tool_call, emit_tool_interrupt_state, emit_tool_result,
    emit_tool_resumed, emit_turn_resumed, emit_workflow_checkpoint_approved,
    emit_workflow_checkpoint_rejected, emit_workflow_checkpoint_requested,
};

use tokio::sync::watch;

use super::provider::AgentTurnDescriptor;
use super::session::AgentRuntimeState;
use super::tools::{execute_tool_call, AgentToolCall};

pub async fn execute_tool_calls(
    sink: &dyn EventSink,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    tool_calls: Vec<AgentToolCall>,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> ExecutedToolBatch {
    let executor_state = runtime_state.clone();
    let tab_id = request.tab_id.clone();
    let project_path = request.project_path.clone();
    let tool_executor: ToolExecutorFn = std::sync::Arc::new(move |call, cancel_rx| {
        let runtime_state = executor_state.clone();
        let tab_id = tab_id.clone();
        let project_path = project_path.clone();
        Box::pin(async move {
            execute_tool_call(&runtime_state, &tab_id, &project_path, call, cancel_rx).await
        })
    });

    agent_core::execute_tool_calls(
        sink,
        runtime_state,
        request,
        tool_calls,
        cancel_rx,
        tool_executor,
    )
    .await
}

#[cfg(test)]
mod compaction_tests {
    use super::*;
    use serde_json::{json, Value};

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
        let mut messages = vec![make_system("system prompt"), make_user("hello")];
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
            assert_ne!(
                role, "tool",
                "tool message should not follow compaction summary"
            );
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
        let messages = vec![make_system("hello"), make_user("world")];
        let total = estimate_messages_tokens(&messages);
        // Each: 4 overhead + estimate_tokens(5 chars) = 4 + 2 = 6
        assert_eq!(total, 12);
    }
}
