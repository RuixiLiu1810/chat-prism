use serde_json::{json, Value};
use tokio::sync::watch;

use super::{
    approval_required_result, error_result, ok_result, tool_arg_string, truncate_preview,
    AgentRuntimeState, AgentToolResult, SHELL_COMMAND_TIMEOUT_SECS, SHELL_OUTPUT_MAX_BYTES,
};
use crate::process_utils;

pub(crate) async fn execute_run_shell_command(
    runtime_state: &AgentRuntimeState,
    tab_id: &str,
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    let command = match tool_arg_string(&args, "command") {
        Ok(value) => value,
        Err(message) => return error_result("run_shell_command", call_id, message),
    };

    let approval = runtime_state
        .check_tool_approval(tab_id, "run_shell_command")
        .await;
    let env_override = std::env::var("CLAUDE_PRISM_AGENT_ALLOW_SHELL")
        .ok()
        .as_deref()
        == Some("1");
    if approval.deny_session {
        return approval_required_result(
            "run_shell_command",
            call_id,
            "run_shell_command is denied for this chat session.".to_string(),
            args,
        );
    }
    if !approval.allow_session && approval.allow_once_remaining == 0 && !env_override {
        return approval_required_result(
            "run_shell_command",
            call_id,
            "run_shell_command requires approval before the command can continue.".to_string(),
            args,
        );
    }

    match process_utils::run_shell_command_cancellable(
        command.clone(),
        project_root.to_string(),
        cancel_rx,
    )
    .await
    {
        Ok(result) => ok_result(
            "run_shell_command",
            call_id,
            json!({
                "command": command,
                "exitCode": result.exit_code,
                "stdout": result.stdout,
                "stderr": result.stderr,
                "stdoutTruncated": result.stdout_truncated,
                "stderrTruncated": result.stderr_truncated,
                "timeoutSecs": SHELL_COMMAND_TIMEOUT_SECS,
                "outputMaxBytes": SHELL_OUTPUT_MAX_BYTES,
            }),
            format!(
                "exit={} stdout={} stderr={}",
                result.exit_code,
                truncate_preview(&result.stdout),
                truncate_preview(&result.stderr)
            ),
        ),
        Err(message) => error_result("run_shell_command", call_id, message),
    }
}
