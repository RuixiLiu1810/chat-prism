use serde_json::{Value, json};
use tokio::sync::watch;

use super::{
    AgentRuntimeState, AgentToolResult, SHELL_COMMAND_TIMEOUT_SECS, SHELL_OUTPUT_MAX_BYTES,
    approval_required_result, error_result, ok_result, tool_arg_string, truncate_preview,
};
use crate::process_utils;

/// Commands that are always blocked, regardless of approval status.
const BLOCKED_SHELL_PATTERNS: &[&str] = &[
    "rm -rf",
    "sudo ",
    "chmod 777",
    "dd ",
    "mkfs",
    "curl | bash",
    "curl|bash",
    "wget | sh",
    "wget|sh",
    "> /dev/",
    ":(){ :",
];

/// Academic-safe commands that are allowed when the user has granted shell approval.
const ALLOWED_SHELL_COMMANDS: &[&str] = &[
    "pdflatex", "xelatex", "lualatex", "bibtex", "biber", "latexmk", "tectonic", "grep", "rg",
    "wc", "cat", "head", "tail", "ls", "find", "diff", "echo", "mkdir", "cp", "mv", "touch",
    "sort", "uniq", "sed", "awk", "python", "python3", "pip", "pip3", "uv", "node", "npm", "npx",
    "git",
];

fn is_blocked_command(command: &str) -> bool {
    let lower = command.to_lowercase();
    BLOCKED_SHELL_PATTERNS
        .iter()
        .any(|pattern| lower.contains(pattern))
}

fn extract_first_command_token(command: &str) -> &str {
    let trimmed = command.trim();
    // Skip env vars like FOO=bar command
    let cmd_start = trimmed
        .split_whitespace()
        .find(|token| !token.contains('='))
        .unwrap_or(trimmed);
    // Handle path prefixes like /usr/bin/grep → grep
    cmd_start.rsplit('/').next().unwrap_or(cmd_start)
}

fn is_allowed_command(command: &str) -> bool {
    let first_token = extract_first_command_token(command);
    ALLOWED_SHELL_COMMANDS
        .iter()
        .any(|allowed| first_token == *allowed)
}

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

    // Hard-reject dangerous commands regardless of approval status
    if is_blocked_command(&command) {
        return error_result(
            "run_shell_command",
            call_id,
            format!(
                "Command blocked for safety: '{}'. This command matches a dangerous pattern and cannot be executed.",
                truncate_preview(&command)
            ),
        );
    }

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

    // After approval, check if the command is in the allowed whitelist
    if !is_allowed_command(&command) && !env_override {
        return approval_required_result(
            "run_shell_command",
            call_id,
            format!(
                "Command '{}' is not in the academic-safe whitelist. Requires explicit approval.",
                extract_first_command_token(&command)
            ),
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
