use serde_json::{json, Value};
use tokio::process::Command;
use tokio::sync::watch;

use super::{
    cancelled_result, command_output_with_cancel, ensure_ripgrep_available, error_result,
    files_preview, is_cancelled, is_document_resource_path, ok_result, resolve_project_path,
    tool_arg_optional_string, tool_arg_string, truncate_file_bytes, AgentToolResult,
};

pub(crate) async fn execute_read_file(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("read_file", call_id);
    }
    let raw_path = match tool_arg_string(&args, "path") {
        Ok(value) => value,
        Err(message) => return error_result("read_file", call_id, message),
    };

    let full_path = match resolve_project_path(project_root, &raw_path) {
        Ok(path) => path,
        Err(message) => return error_result("read_file", call_id, message),
    };

    if is_document_resource_path(&raw_path) {
        return error_result(
            "read_file",
            call_id,
            format!(
                "{} is a document resource, not a plain text file. Use read_document instead.",
                raw_path
            ),
        );
    }

    let bytes = match tokio::fs::read(&full_path).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return error_result(
                "read_file",
                call_id,
                format!("Failed to read {}: {}", raw_path, err),
            )
        }
    };

    let (slice, truncated) = truncate_file_bytes(&bytes);
    let content = String::from_utf8_lossy(slice).to_string();
    let preview = format!(
        "{}{}",
        content,
        if truncated { "\n...[truncated]" } else { "" }
    );

    ok_result(
        "read_file",
        call_id,
        json!({
            "path": raw_path,
            "content": content,
            "truncated": truncated,
            "byteCount": bytes.len(),
        }),
        preview,
    )
}

pub(crate) async fn execute_list_files(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    let path = tool_arg_optional_string(&args, "path").unwrap_or_else(|| ".".to_string());
    if let Err(result) = ensure_ripgrep_available("list_files", call_id, project_root).await {
        return result;
    }
    let search_root = match resolve_project_path(project_root, &path) {
        Ok(path) => path,
        Err(message) => return error_result("list_files", call_id, message),
    };

    let mut command = Command::new("rg");
    command
        .arg("--files")
        .arg(&search_root)
        .current_dir(project_root);

    let output = match command_output_with_cancel(
        command,
        cancel_rx,
        "list_files",
        call_id,
        "Failed to run rg --files",
    )
    .await
    {
        Ok(output) => output,
        Err(result) => return result,
    };

    if !output.status.success() {
        return error_result(
            "list_files",
            call_id,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        );
    }

    let mut files = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let path = std::path::Path::new(line);
            path.strip_prefix(project_root)
                .ok()
                .map(|relative| {
                    relative
                        .to_string_lossy()
                        .trim_start_matches('/')
                        .to_string()
                })
                .unwrap_or_else(|| line.to_string())
        })
        .collect::<Vec<_>>();
    files.sort();
    let truncated = files.len() > super::MAX_LISTED_FILES;
    if truncated {
        files.truncate(super::MAX_LISTED_FILES);
    }

    ok_result(
        "list_files",
        call_id,
        json!({
            "path": path,
            "files": files,
            "truncated": truncated,
        }),
        files_preview("Files", &files, truncated),
    )
}

pub(crate) async fn execute_search_project(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    let query = match tool_arg_string(&args, "query") {
        Ok(value) => value,
        Err(message) => return error_result("search_project", call_id, message),
    };
    let path = tool_arg_optional_string(&args, "path").unwrap_or_else(|| ".".to_string());
    if let Err(result) = ensure_ripgrep_available("search_project", call_id, project_root).await {
        return result;
    }
    let search_root = match resolve_project_path(project_root, &path) {
        Ok(path) => path,
        Err(message) => return error_result("search_project", call_id, message),
    };

    let mut command = Command::new("rg");
    command
        .arg("-n")
        .arg("--no-heading")
        .arg("--color")
        .arg("never")
        .arg(&query)
        .arg(&search_root)
        .current_dir(project_root);

    let output = match command_output_with_cancel(
        command,
        cancel_rx,
        "search_project",
        call_id,
        "Failed to run ripgrep",
    )
    .await
    {
        Ok(output) => output,
        Err(result) => return result,
    };

    if output.status.code().unwrap_or(-1) > 1 {
        return error_result(
            "search_project",
            call_id,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        );
    }

    let mut matches = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let truncated = matches.len() > super::MAX_SEARCH_LINES;
    if truncated {
        matches.truncate(super::MAX_SEARCH_LINES);
    }

    ok_result(
        "search_project",
        call_id,
        json!({
            "query": query,
            "path": path,
            "matches": matches,
            "truncated": truncated,
        }),
        files_preview("Matches", &matches, truncated),
    )
}
