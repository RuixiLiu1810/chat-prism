use std::path::PathBuf;

use serde_json::{json, Value};
use tokio::sync::watch;

use super::{
    approval_bucket_for_tool, approval_required_edit_result, build_review_artifact,
    cancelled_result, error_result, is_cancelled, ok_result, read_existing_file_for_edit,
    replace_by_anchor, replace_unique_exact, replace_unique_with_trimmed_fallback,
    resolve_project_path, tool_arg_optional_string, tool_arg_string, AgentRuntimeState,
    AgentToolResult,
};

async fn materialize_reviewable_edit(
    runtime_state: &AgentRuntimeState,
    tab_id: &str,
    tool_name: &str,
    call_id: &str,
    raw_path: String,
    full_path: PathBuf,
    old_content: String,
    new_content: String,
    args: Value,
    preview_label: String,
    extra: Value,
    cancel_rx: Option<&watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx) {
        return cancelled_result(tool_name, call_id);
    }
    let approval_tool_name = approval_bucket_for_tool(tool_name);
    let approval = runtime_state
        .check_tool_approval(tab_id, approval_tool_name)
        .await;
    let env_override = std::env::var("CLAUDE_PRISM_AGENT_ALLOW_WRITE")
        .ok()
        .as_deref()
        == Some("1");

    if approval.deny_session {
        return approval_required_edit_result(
            tool_name,
            approval_tool_name,
            call_id,
            &format!("{} is denied for this chat session.", approval_tool_name),
            args,
            &raw_path,
            &full_path,
            &old_content,
            &new_content,
            extra,
        );
    }

    if !approval.allow_session && approval.allow_once_remaining == 0 && !env_override {
        return approval_required_edit_result(
            tool_name,
            approval_tool_name,
            call_id,
            "File editing requires approval before the edit can be applied. Review is ready in the diff panel.",
            args,
            &raw_path,
            &full_path,
            &old_content,
            &new_content,
            extra,
        );
    }

    if let Some(parent) = full_path.parent() {
        if is_cancelled(cancel_rx) {
            return cancelled_result(tool_name, call_id);
        }
        if let Err(err) = tokio::fs::create_dir_all(parent).await {
            return error_result(
                tool_name,
                call_id,
                format!(
                    "Failed to create parent directory for {}: {}",
                    raw_path, err
                ),
            );
        }
    }

    if is_cancelled(cancel_rx) {
        return cancelled_result(tool_name, call_id);
    }
    if let Err(err) = tokio::fs::write(&full_path, new_content.as_bytes()).await {
        return error_result(
            tool_name,
            call_id,
            format!("Failed to write {}: {}", raw_path, err),
        );
    }

    ok_result(
        tool_name,
        call_id,
        build_review_artifact(
            tool_name,
            approval_tool_name,
            &raw_path,
            &full_path,
            &old_content,
            &new_content,
            true,
            &extra,
        )
        .to_content_value(false, None, None, extra),
        preview_label,
    )
}

pub(crate) async fn execute_replace_selected_text(
    runtime_state: &AgentRuntimeState,
    tab_id: &str,
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    let raw_path = match tool_arg_string(&args, "path") {
        Ok(value) => value,
        Err(message) => return error_result("replace_selected_text", call_id, message),
    };
    let expected_selected_text = match tool_arg_string(&args, "expected_selected_text") {
        Ok(value) => value,
        Err(message) => return error_result("replace_selected_text", call_id, message),
    };
    let replacement_text = match tool_arg_string(&args, "replacement_text") {
        Ok(value) => value,
        Err(message) => return error_result("replace_selected_text", call_id, message),
    };
    let selection_anchor = tool_arg_optional_string(&args, "selection_anchor");

    let full_path = match resolve_project_path(project_root, &raw_path) {
        Ok(path) => path,
        Err(message) => return error_result("replace_selected_text", call_id, message),
    };

    let old_content = match read_existing_file_for_edit(
        "replace_selected_text",
        call_id,
        &raw_path,
        &full_path,
        cancel_rx.as_ref(),
    )
    .await
    {
        Ok(content) => content,
        Err(result) => return result,
    };

    let new_content = match selection_anchor.as_deref() {
        Some(anchor) => match replace_by_anchor(
            &raw_path,
            &old_content,
            &expected_selected_text,
            &replacement_text,
            anchor,
        ) {
            Ok(Some(updated)) => updated,
            Ok(None) => {
                match replace_unique_exact(&old_content, &expected_selected_text, &replacement_text)
                {
                    Ok(updated) => updated,
                    Err(message) => {
                        return error_result(
                            "replace_selected_text",
                            call_id,
                            format!(
                                "Selection anchor did not match the expected text, and fallback exact replacement failed: {}",
                                message
                            ),
                        );
                    }
                }
            }
            Err(message) => return error_result("replace_selected_text", call_id, message),
        },
        None => {
            match replace_unique_exact(&old_content, &expected_selected_text, &replacement_text) {
                Ok(updated) => updated,
                Err(message) => return error_result("replace_selected_text", call_id, message),
            }
        }
    };

    materialize_reviewable_edit(
        runtime_state,
        tab_id,
        "replace_selected_text",
        call_id,
        raw_path.clone(),
        full_path,
        old_content,
        new_content,
        args,
        format!("Updated selected text in {}", raw_path),
        json!({
            "editKind": "replace_selected_text",
            "selectionAnchor": selection_anchor,
            "expectedSelectedText": expected_selected_text,
            "replacementText": replacement_text,
        }),
        cancel_rx.as_ref(),
    )
    .await
}

pub(crate) async fn execute_apply_text_patch(
    runtime_state: &AgentRuntimeState,
    tab_id: &str,
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    let raw_path = match tool_arg_string(&args, "path") {
        Ok(value) => value,
        Err(message) => return error_result("apply_text_patch", call_id, message),
    };
    let expected_old_text = match tool_arg_string(&args, "expected_old_text") {
        Ok(value) => value,
        Err(message) => return error_result("apply_text_patch", call_id, message),
    };
    let new_text = match tool_arg_string(&args, "new_text") {
        Ok(value) => value,
        Err(message) => return error_result("apply_text_patch", call_id, message),
    };

    let full_path = match resolve_project_path(project_root, &raw_path) {
        Ok(path) => path,
        Err(message) => return error_result("apply_text_patch", call_id, message),
    };

    let old_content = match read_existing_file_for_edit(
        "apply_text_patch",
        call_id,
        &raw_path,
        &full_path,
        cancel_rx.as_ref(),
    )
    .await
    {
        Ok(content) => content,
        Err(result) => return result,
    };

    let (new_content, used_trimmed_fallback) =
        match replace_unique_with_trimmed_fallback(&old_content, &expected_old_text, &new_text) {
            Ok(updated) => updated,
            Err(message) => return error_result("apply_text_patch", call_id, message),
        };

    materialize_reviewable_edit(
        runtime_state,
        tab_id,
        "apply_text_patch",
        call_id,
        raw_path.clone(),
        full_path,
        old_content,
        new_content,
        args,
        format!("Applied text patch to {}", raw_path),
        json!({
            "editKind": "apply_text_patch",
            "expectedOldText": expected_old_text,
            "newText": new_text,
            "usedTrimmedFallback": used_trimmed_fallback,
        }),
        cancel_rx.as_ref(),
    )
    .await
}

pub(crate) async fn execute_write_file(
    runtime_state: &AgentRuntimeState,
    tab_id: &str,
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    let raw_path = match tool_arg_string(&args, "path") {
        Ok(value) => value,
        Err(message) => return error_result("write_file", call_id, message),
    };
    let content = match tool_arg_string(&args, "content") {
        Ok(value) => value,
        Err(message) => return error_result("write_file", call_id, message),
    };

    let full_path = match resolve_project_path(project_root, &raw_path) {
        Ok(path) => path,
        Err(message) => return error_result("write_file", call_id, message),
    };

    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("write_file", call_id);
    }
    let old_content = match tokio::fs::read_to_string(&full_path).await {
        Ok(existing) => existing,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            return error_result(
                "write_file",
                call_id,
                format!("Failed to read existing {} before write: {}", raw_path, err),
            );
        }
    };

    materialize_reviewable_edit(
        runtime_state,
        tab_id,
        "write_file",
        call_id,
        raw_path.clone(),
        full_path,
        old_content,
        content.clone(),
        args,
        format!("Wrote {}", raw_path),
        json!({
            "editKind": "write_file",
            "charCount": content.chars().count(),
        }),
        cancel_rx.as_ref(),
    )
    .await
}
