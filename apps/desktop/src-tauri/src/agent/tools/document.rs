use serde_json::{json, Value};
use tokio::sync::watch;

use super::{
    cancelled_result, error_result, is_cancelled, load_document_runtime_content, ok_result,
    resolve_project_path, tool_arg_optional_string, tool_arg_optional_usize, tool_arg_string,
    AgentToolResult, DocumentArtifact,
};
use crate::agent::document_artifacts::{
    find_relevant_document_matches, format_document_matches_preview, is_document_resource_path,
};

pub(crate) async fn execute_inspect_resource(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("inspect_resource", call_id);
    }
    let raw_path = match tool_arg_string(&args, "path") {
        Ok(value) => value,
        Err(message) => return error_result("inspect_resource", call_id, message),
    };
    let full_path = match resolve_project_path(project_root, &raw_path) {
        Ok(path) => path,
        Err(message) => return error_result("inspect_resource", call_id, message),
    };
    let metadata = match tokio::fs::metadata(&full_path).await {
        Ok(metadata) => metadata,
        Err(err) => {
            return error_result(
                "inspect_resource",
                call_id,
                format!("Failed to inspect {}: {}", raw_path, err),
            )
        }
    };

    let kind = super::resource_kind_from_path(&raw_path).to_string();
    let runtime_content = load_document_runtime_content(project_root, &raw_path, cancel_rx.clone())
        .await
        .ok();
    let extraction_status = runtime_content
        .as_ref()
        .map(|artifact| artifact.extraction_status.clone())
        .unwrap_or_else(|| {
            if is_document_resource_path(&raw_path) {
                "missing_artifact".to_string()
            } else {
                "plain_text".to_string()
            }
        });
    let source_type = runtime_content
        .as_ref()
        .map(|artifact| artifact.source_type.clone())
        .unwrap_or_else(|| {
            full_path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("file")
                .to_string()
        });
    let preview = if let Some(artifact) = runtime_content.as_ref() {
        format!(
            "Resource {} is a {} with ingestion status {} and {} indexed segments{}.",
            raw_path,
            artifact.kind,
            artifact.extraction_status,
            artifact.segments.len(),
            if artifact.fallback_used {
                " (using internal pdftotext fallback)"
            } else {
                ""
            }
        )
    } else if is_document_resource_path(&raw_path) {
        format!(
            "Resource {} is a {} but no ingested document artifact is available yet.",
            raw_path, kind
        )
    } else {
        format!("Resource {} is a plain text file.", raw_path)
    };

    ok_result(
        "inspect_resource",
        call_id,
        json!({
            "path": raw_path,
            "kind": kind,
            "sourceType": source_type,
            "artifactReady": runtime_content.is_some(),
            "extractionStatus": extraction_status,
            "pageCount": runtime_content.as_ref().and_then(|value| value.page_count),
            "fallbackUsed": runtime_content
                .as_ref()
                .map(|value| value.fallback_used)
                .unwrap_or(false),
            "byteCount": metadata.len(),
        }),
        preview,
    )
}

pub(crate) async fn execute_read_document_excerpt(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("read_document_excerpt", call_id);
    }
    let raw_path = match tool_arg_string(&args, "path") {
        Ok(value) => value,
        Err(message) => return error_result("read_document_excerpt", call_id, message),
    };
    if !is_document_resource_path(&raw_path) {
        return error_result(
            "read_document_excerpt",
            call_id,
            format!(
                "{} is not a PDF/DOCX document resource. Use read_file for plain text files instead.",
                raw_path
            ),
        );
    }

    let runtime_content =
        match load_document_runtime_content(project_root, &raw_path, cancel_rx).await {
            Ok(content) => content,
            Err(message) => return error_result("read_document_excerpt", call_id, message),
        };

    ok_result(
        "read_document_excerpt",
        call_id,
        json!({
            "path": raw_path,
            "kind": runtime_content.kind,
            "sourceType": runtime_content.source_type,
            "extractionStatus": runtime_content.extraction_status,
            "excerpt": runtime_content.excerpt,
            "pageCount": runtime_content.page_count,
            "fallbackUsed": runtime_content.fallback_used,
        }),
        runtime_content.excerpt,
    )
}

pub(crate) async fn execute_read_document(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result("read_document", call_id);
    }
    let raw_path = match tool_arg_string(&args, "path") {
        Ok(value) => value,
        Err(message) => return error_result("read_document", call_id, message),
    };
    let query = tool_arg_optional_string(&args, "query")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let limit = tool_arg_optional_usize(&args, "limit")
        .unwrap_or(3)
        .clamp(1, 8);

    if !is_document_resource_path(&raw_path) {
        return error_result(
            "read_document",
            call_id,
            format!(
                "{} is not a PDF/DOCX document resource. Use read_file for plain text files instead.",
                raw_path
            ),
        );
    }

    let runtime_content =
        match load_document_runtime_content(project_root, &raw_path, cancel_rx).await {
            Ok(content) => content,
            Err(message) => return error_result("read_document", call_id, message),
        };
    let artifact = runtime_content_to_document_artifact(&raw_path, &runtime_content);
    let matches = query
        .as_ref()
        .map(|text| find_relevant_document_matches(&artifact, text, limit))
        .unwrap_or_default();
    let preview = match query.as_ref() {
        Some(text) => {
            if matches.is_empty() {
                format!(
                    "Read document {} but found no relevant evidence for '{}'.",
                    raw_path, text
                )
            } else {
                format_document_matches_preview(
                    &raw_path,
                    &artifact.source_type,
                    &matches,
                    "Document evidence",
                )
            }
        }
        None => runtime_content.excerpt.clone(),
    };

    ok_result(
        "read_document",
        call_id,
        json!({
            "path": raw_path,
            "kind": runtime_content.kind,
            "sourceType": runtime_content.source_type,
            "extractionStatus": runtime_content.extraction_status,
            "excerpt": runtime_content.excerpt,
            "query": query,
            "matches": matches,
            "pageCount": runtime_content.page_count,
            "fallbackUsed": runtime_content.fallback_used,
        }),
        preview,
    )
}

pub(crate) async fn execute_search_document_text(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    execute_document_search_like_tool(
        "search_document_text",
        "Document matches",
        project_root,
        call_id,
        args,
        cancel_rx,
    )
    .await
}

pub(crate) async fn execute_get_document_evidence(
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    execute_document_search_like_tool(
        "get_document_evidence",
        "Supporting evidence",
        project_root,
        call_id,
        args,
        cancel_rx,
    )
    .await
}

async fn execute_document_search_like_tool(
    tool_name: &str,
    preview_prefix: &str,
    project_root: &str,
    call_id: &str,
    args: Value,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return cancelled_result(tool_name, call_id);
    }
    let raw_path = match tool_arg_string(&args, "path") {
        Ok(value) => value,
        Err(message) => return error_result(tool_name, call_id, message),
    };
    let query = match tool_arg_string(&args, "query") {
        Ok(value) => value,
        Err(message) => return error_result(tool_name, call_id, message),
    };
    let limit = tool_arg_optional_usize(&args, "limit")
        .unwrap_or(3)
        .clamp(1, 8);

    if !is_document_resource_path(&raw_path) {
        return error_result(
            tool_name,
            call_id,
            format!(
                "{} is not a PDF/DOCX document resource. Use search_project for code/text files or read_file for a known text path.",
                raw_path
            ),
        );
    }

    let runtime_content =
        match load_document_runtime_content(project_root, &raw_path, cancel_rx).await {
            Ok(content) => content,
            Err(message) => return error_result(tool_name, call_id, message),
        };
    let artifact = runtime_content_to_document_artifact(&raw_path, &runtime_content);
    let matches = find_relevant_document_matches(&artifact, &query, limit);
    let preview =
        format_document_matches_preview(&raw_path, &artifact.source_type, &matches, preview_prefix);

    ok_result(
        tool_name,
        call_id,
        json!({
            "path": raw_path,
            "query": query,
            "kind": artifact.kind,
            "sourceType": artifact.source_type,
            "extractionStatus": artifact.extraction_status,
            "matches": matches,
            "pageCount": artifact.page_count,
            "fallbackUsed": runtime_content.fallback_used,
        }),
        preview,
    )
}

fn runtime_content_to_document_artifact(
    file_path: &str,
    runtime_content: &super::DocumentRuntimeContent,
) -> DocumentArtifact {
    DocumentArtifact {
        version: 2,
        file_path: file_path.to_string(),
        absolute_path: String::new(),
        source_type: runtime_content.source_type.clone(),
        kind: runtime_content.kind.clone(),
        extraction_status: runtime_content.extraction_status.clone(),
        excerpt: runtime_content.excerpt.clone(),
        searchable_text: runtime_content.searchable_text.clone(),
        segments: runtime_content.segments.clone(),
        page_count: runtime_content.page_count,
        metadata: json!({}),
    }
}
