use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::process::Command;
use tokio::sync::watch;

use crate::process_utils;

#[path = "tools/document.rs"]
mod document;
#[path = "tools/edit.rs"]
mod edit;
#[path = "tools/literature.rs"]
mod literature;
#[path = "tools/review.rs"]
mod review;
#[path = "tools/shell.rs"]
mod shell;
#[path = "tools/workspace.rs"]
mod workspace;
#[path = "tools/writing.rs"]
mod writing;

use super::document_artifacts::{
    is_document_resource_path, load_document_artifact, resource_kind_from_path, DocumentArtifact,
    DocumentArtifactSegment,
};
use super::provider::AgentTaskKind;
use super::review_runtime::AgentReviewArtifact;
use super::session::AgentRuntimeState;
use super::AGENT_CANCELLED_MESSAGE;

const MAX_FILE_BYTES: usize = 200_000;
const MAX_LISTED_FILES: usize = 500;
const MAX_SEARCH_LINES: usize = 200;
const MAX_PREVIEW_CHARS: usize = 240;
const SHELL_COMMAND_TIMEOUT_SECS: u64 = 30;
const SHELL_OUTPUT_MAX_BYTES: usize = 32_000;
const DOCUMENT_FALLBACK_TIMEOUT_SECS: u64 = 30;
const DOCUMENT_FALLBACK_MAX_BYTES: usize = 400_000;
const DOCUMENT_EXCERPT_MAX_CHARS: usize = 6_000;

pub(crate) use document::{
    execute_get_document_evidence, execute_inspect_resource, execute_read_document,
    execute_read_document_excerpt, execute_search_document_text,
};
pub(crate) use edit::{
    execute_apply_text_patch, execute_replace_selected_text, execute_write_file,
};
pub(crate) use literature::{
    execute_analyze_paper, execute_compare_papers, execute_extract_methodology,
    execute_search_literature, execute_synthesize_evidence,
};
pub(crate) use review::{
    execute_check_statistics, execute_generate_response_letter, execute_review_manuscript,
    execute_track_revisions, execute_verify_references,
};
pub(crate) use shell::execute_run_shell_command;
pub(crate) use workspace::{execute_list_files, execute_read_file, execute_search_project};
pub(crate) use writing::{
    execute_check_consistency, execute_draft_section, execute_generate_abstract,
    execute_insert_citation, execute_restructure_outline,
};

#[derive(Debug, Clone)]
struct SelectionAnchor {
    path: String,
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
}

#[derive(Debug, Clone)]
struct DocumentRuntimeContent {
    kind: String,
    source_type: String,
    extraction_status: String,
    excerpt: String,
    searchable_text: String,
    segments: Vec<DocumentArtifactSegment>,
    page_count: Option<usize>,
    fallback_used: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCapabilityClass {
    ReadTextFile,
    ReadDocument,
    SearchDocument,
    InspectResource,
    LiteratureAnalysis,
    DraftWriting,
    ReviewWriting,
    EditPatch,
    EditWrite,
    ListWorkspace,
    SearchWorkspace,
    ExecuteShell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolResourceScope {
    TextFile,
    Document,
    Workspace,
    Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolApprovalPolicy {
    Never,
    SessionScoped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolReviewPolicy {
    None,
    DiffRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSuspendBehavior {
    None,
    SuspendOnApproval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolResultShape {
    TextRead,
    DocumentExcerpt,
    DocumentSearch,
    ResourceInfo,
    LiteratureOutput,
    WritingOutput,
    WorkspaceSearch,
    ReviewArtifact,
    CommandOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentToolContract {
    pub capability_class: ToolCapabilityClass,
    pub resource_scope: ToolResourceScope,
    pub approval_policy: ToolApprovalPolicy,
    pub review_policy: ToolReviewPolicy,
    pub suspend_behavior: ToolSuspendBehavior,
    pub result_shape: ToolResultShape,
    pub parallel_safe: bool,
    pub approval_bucket: &'static str,
}

pub fn tool_contract(tool_name: &str) -> AgentToolContract {
    match tool_name {
        "read_file" => AgentToolContract {
            capability_class: ToolCapabilityClass::ReadTextFile,
            resource_scope: ToolResourceScope::TextFile,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::TextRead,
            parallel_safe: true,
            approval_bucket: "read_file",
        },
        "read_document" => AgentToolContract {
            capability_class: ToolCapabilityClass::ReadDocument,
            resource_scope: ToolResourceScope::Document,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::DocumentExcerpt,
            parallel_safe: true,
            approval_bucket: "read_document",
        },
        "search_literature"
        | "analyze_paper"
        | "compare_papers"
        | "synthesize_evidence"
        | "extract_methodology" => AgentToolContract {
            capability_class: ToolCapabilityClass::LiteratureAnalysis,
            resource_scope: ToolResourceScope::Document,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::LiteratureOutput,
            parallel_safe: true,
            approval_bucket: "literature_analysis",
        },
        "inspect_resource" => AgentToolContract {
            capability_class: ToolCapabilityClass::InspectResource,
            resource_scope: ToolResourceScope::Document,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::ResourceInfo,
            parallel_safe: true,
            approval_bucket: "inspect_resource",
        },
        "read_document_excerpt" => AgentToolContract {
            capability_class: ToolCapabilityClass::ReadDocument,
            resource_scope: ToolResourceScope::Document,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::DocumentExcerpt,
            parallel_safe: true,
            approval_bucket: "read_document_excerpt",
        },
        "search_document_text" => AgentToolContract {
            capability_class: ToolCapabilityClass::SearchDocument,
            resource_scope: ToolResourceScope::Document,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::DocumentSearch,
            parallel_safe: true,
            approval_bucket: "search_document_text",
        },
        "get_document_evidence" => AgentToolContract {
            capability_class: ToolCapabilityClass::SearchDocument,
            resource_scope: ToolResourceScope::Document,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::DocumentSearch,
            parallel_safe: true,
            approval_bucket: "get_document_evidence",
        },
        "draft_section" | "restructure_outline" | "generate_abstract" => AgentToolContract {
            capability_class: ToolCapabilityClass::DraftWriting,
            resource_scope: ToolResourceScope::Workspace,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::WritingOutput,
            parallel_safe: true,
            approval_bucket: "writing_draft",
        },
        "check_consistency" | "insert_citation" => AgentToolContract {
            capability_class: ToolCapabilityClass::ReviewWriting,
            resource_scope: ToolResourceScope::Workspace,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::WritingOutput,
            parallel_safe: true,
            approval_bucket: "writing_review",
        },
        "review_manuscript"
        | "check_statistics"
        | "verify_references"
        | "generate_response_letter"
        | "track_revisions" => AgentToolContract {
            capability_class: ToolCapabilityClass::ReviewWriting,
            resource_scope: ToolResourceScope::Workspace,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::WritingOutput,
            parallel_safe: true,
            approval_bucket: "peer_review",
        },
        "replace_selected_text" | "apply_text_patch" => AgentToolContract {
            capability_class: ToolCapabilityClass::EditPatch,
            resource_scope: ToolResourceScope::TextFile,
            approval_policy: ToolApprovalPolicy::SessionScoped,
            review_policy: ToolReviewPolicy::DiffRequired,
            suspend_behavior: ToolSuspendBehavior::SuspendOnApproval,
            result_shape: ToolResultShape::ReviewArtifact,
            parallel_safe: false,
            approval_bucket: "patch_file",
        },
        "write_file" => AgentToolContract {
            capability_class: ToolCapabilityClass::EditWrite,
            resource_scope: ToolResourceScope::TextFile,
            approval_policy: ToolApprovalPolicy::SessionScoped,
            review_policy: ToolReviewPolicy::DiffRequired,
            suspend_behavior: ToolSuspendBehavior::SuspendOnApproval,
            result_shape: ToolResultShape::ReviewArtifact,
            parallel_safe: false,
            approval_bucket: "write_file",
        },
        "list_files" => AgentToolContract {
            capability_class: ToolCapabilityClass::ListWorkspace,
            resource_scope: ToolResourceScope::Workspace,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::WorkspaceSearch,
            parallel_safe: true,
            approval_bucket: "list_files",
        },
        "search_project" => AgentToolContract {
            capability_class: ToolCapabilityClass::SearchWorkspace,
            resource_scope: ToolResourceScope::Workspace,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::WorkspaceSearch,
            parallel_safe: true,
            approval_bucket: "search_project",
        },
        "run_shell_command" => AgentToolContract {
            capability_class: ToolCapabilityClass::ExecuteShell,
            resource_scope: ToolResourceScope::Shell,
            approval_policy: ToolApprovalPolicy::SessionScoped,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::SuspendOnApproval,
            result_shape: ToolResultShape::CommandOutput,
            parallel_safe: false,
            approval_bucket: "run_shell_command",
        },
        _ => AgentToolContract {
            capability_class: ToolCapabilityClass::SearchWorkspace,
            resource_scope: ToolResourceScope::Workspace,
            approval_policy: ToolApprovalPolicy::Never,
            review_policy: ToolReviewPolicy::None,
            suspend_behavior: ToolSuspendBehavior::None,
            result_shape: ToolResultShape::WorkspaceSearch,
            parallel_safe: false,
            approval_bucket: "unknown",
        },
    }
}

pub fn is_reviewable_edit_tool(tool_name: &str) -> bool {
    tool_contract(tool_name).review_policy == ToolReviewPolicy::DiffRequired
}

pub fn is_parallel_safe_tool(tool_name: &str) -> bool {
    tool_contract(tool_name).parallel_safe
}

pub fn is_document_tool_name(tool_name: &str) -> bool {
    matches!(
        tool_contract(tool_name).resource_scope,
        ToolResourceScope::Document
    )
}

pub fn tool_display_kind(tool_name: &str) -> &'static str {
    match tool_contract(tool_name).capability_class {
        ToolCapabilityClass::ReadTextFile => "text_read",
        ToolCapabilityClass::ReadDocument => "document_read",
        ToolCapabilityClass::SearchDocument => "document_search",
        ToolCapabilityClass::InspectResource => "resource_info",
        ToolCapabilityClass::LiteratureAnalysis => "literature_analysis",
        ToolCapabilityClass::DraftWriting => "writing_draft",
        ToolCapabilityClass::ReviewWriting => "writing_review",
        ToolCapabilityClass::EditPatch => "edit_patch",
        ToolCapabilityClass::EditWrite => "edit_write",
        ToolCapabilityClass::ListWorkspace | ToolCapabilityClass::SearchWorkspace => {
            "workspace_search"
        }
        ToolCapabilityClass::ExecuteShell => "shell_command",
    }
}

fn approval_bucket_for_tool(tool_name: &str) -> &str {
    tool_contract(tool_name).approval_bucket
}

fn make_tool_spec(name: &str, description: &str, input_schema: Value) -> AgentToolSpec {
    AgentToolSpec {
        name: name.to_string(),
        description: description.to_string(),
        input_schema,
        contract: tool_contract(name),
    }
}

fn writing_tools_enabled() -> bool {
    std::env::var("PRISM_AGENT_WRITING_TOOLS")
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            normalized != "0" && normalized != "false" && normalized != "off"
        })
        .unwrap_or(true)
}

#[derive(Debug, Clone)]
pub struct AgentToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub contract: AgentToolContract,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolCall {
    pub tool_name: String,
    pub call_id: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolResult {
    pub tool_name: String,
    pub call_id: String,
    pub is_error: bool,
    pub content: Value,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolResultDisplayContent {
    pub kind: String,
    pub display_kind: String,
    pub tool_name: String,
    pub status: String,
    pub text_preview: String,
    pub is_error: bool,
    pub target_path: Option<String>,
    pub command: Option<String>,
    pub query: Option<String>,
    pub approval_required: bool,
    pub review_ready: bool,
    pub approval_reason: Option<String>,
    pub approval_tool_name: Option<String>,
    pub written: Option<bool>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolExecutionPolicyContext {
    pub task_kind: AgentTaskKind,
    pub has_binary_attachment_context: bool,
}

pub fn default_tool_specs() -> Vec<AgentToolSpec> {
    build_default_tool_specs(writing_tools_enabled())
}

fn build_default_tool_specs(include_writing_tools: bool) -> Vec<AgentToolSpec> {
    let mut specs = vec![
        make_tool_spec(
            "read_file",
            "Read a text file from the current project. Supported: source code, markdown, JSON, CSV, and plain text. Do not use this tool for PDF, DOCX, images, or other binary resources.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project-relative file path." }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        ),
        make_tool_spec(
            "read_document",
            "Read an attached or project document resource (PDF/DOCX) using the runtime ingestion pipeline. Provide path always, and optionally query to extract targeted evidence snippets.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project-relative path for the document resource." },
                    "query": { "type": "string", "description": "Optional question or search query for targeted evidence." },
                    "limit": { "type": "integer", "description": "Optional maximum number of evidence snippets to return when query is provided." }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        ),
        make_tool_spec(
            "search_literature",
            "Search academic literature across multiple providers (Semantic Scholar, OpenAlex, Crossref, PubMed). Supports optional MeSH expansion and year filters. Returns structured citation candidates.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Literature query describing the research topic, claim, or question." },
                    "limit": { "type": "integer", "description": "Optional maximum number of merged results (default 10)." },
                    "mesh_expansion": { "type": "boolean", "description": "Whether to include MeSH-oriented query expansions (default true)." },
                    "min_year": { "type": "integer", "description": "Optional lower publication year bound." },
                    "max_year": { "type": "integer", "description": "Optional upper publication year bound." }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        make_tool_spec(
            "analyze_paper",
            "Analyze an ingested PDF/DOCX paper and return objective, methods, findings, limitations, and relevance.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project-relative path to PDF/DOCX resource." },
                    "focus": { "type": "string", "description": "Optional focus question to score relevance." },
                    "max_items": { "type": "integer", "description": "Optional max number of extracted method/finding/limitation items." }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        ),
        make_tool_spec(
            "compare_papers",
            "Compare multiple papers and return shared findings, conflicting signals, and methodology differences.",
            json!({
                "type": "object",
                "properties": {
                    "papers": {
                        "type": "array",
                        "description": "Array of paper paths or objects with path field.",
                        "items": {
                            "oneOf": [
                                { "type": "string" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "path": { "type": "string" }
                                    },
                                    "required": ["path"],
                                    "additionalProperties": false
                                }
                            ]
                        }
                    },
                    "focus": { "type": "string", "description": "Optional comparison focus question." }
                },
                "required": ["papers"],
                "additionalProperties": false
            }),
        ),
        make_tool_spec(
            "synthesize_evidence",
            "Synthesize evidence from multiple papers into theme-organized blocks with source-linked support.",
            json!({
                "type": "object",
                "properties": {
                    "papers": {
                        "type": "array",
                        "description": "Array of paper paths or objects with path field.",
                        "items": {
                            "oneOf": [
                                { "type": "string" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "path": { "type": "string" }
                                    },
                                    "required": ["path"],
                                    "additionalProperties": false
                                }
                            ]
                        }
                    },
                    "focus": { "type": "string", "description": "Optional synthesis focus question." }
                },
                "required": ["papers"],
                "additionalProperties": false
            }),
        ),
        make_tool_spec(
            "extract_methodology",
            "Extract structured methodology fields from a paper: study design, sample, intervention, endpoints, and statistics.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project-relative path to PDF/DOCX resource." }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        ),
    ];

    if include_writing_tools {
        specs.extend([
            make_tool_spec(
                "draft_section",
                "Draft a manuscript section from structured key points. Use this for first-pass writing in English academic style, then refine with follow-up edits.",
                json!({
                    "type": "object",
                    "properties": {
                        "section_type": { "type": "string", "description": "Section type such as introduction, methods, results, discussion, or conclusion." },
                        "key_points": {
                            "description": "Core points to include. Pass either an array of strings or a newline-delimited string.",
                            "oneOf": [
                                { "type": "array", "items": { "type": "string" } },
                                { "type": "string" }
                            ]
                        },
                        "tone": { "type": "string", "description": "Optional writing tone (for example: formal, concise, persuasive)." },
                        "target_words": { "type": "integer", "description": "Optional target word count." },
                        "citation_keys": { "type": "array", "items": { "type": "string" }, "description": "Optional citation keys to weave into the draft." },
                        "output_format": { "type": "string", "description": "Optional output format: markdown | latex | plain." }
                    },
                    "required": ["section_type", "key_points"],
                    "additionalProperties": false
                }),
            ),
            make_tool_spec(
                "restructure_outline",
                "Restructure a manuscript outline into a coherent section order with rationale per section.",
                json!({
                    "type": "object",
                    "properties": {
                        "outline": { "type": "string", "description": "Optional free-form outline text." },
                        "sections": { "type": "array", "items": { "type": "string" }, "description": "Optional explicit section list." },
                        "manuscript_type": { "type": "string", "description": "Optional manuscript type: imrad | review | case_report | methods." }
                    },
                    "additionalProperties": false
                }),
            ),
            make_tool_spec(
                "check_consistency",
                "Run consistency checks on manuscript text: abbreviations, numbering, terminology, placeholders, and citation marker style.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Optional project-relative path of the manuscript file." },
                        "text": { "type": "string", "description": "Optional inline manuscript text if path is not provided." }
                    },
                    "additionalProperties": false
                }),
            ),
            make_tool_spec(
                "generate_abstract",
                "Generate a draft abstract from manuscript text with optional structured mode and word limit.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Optional project-relative manuscript path." },
                        "text": { "type": "string", "description": "Optional inline manuscript text if path is not provided." },
                        "structured": { "type": "boolean", "description": "Whether to output Background/Methods/Results/Conclusions sections." },
                        "word_limit": { "type": "integer", "description": "Optional word limit for the abstract." }
                    },
                    "additionalProperties": false
                }),
            ),
            make_tool_spec(
                "insert_citation",
                "Insert a citation marker into text using a provided citation key and style (latex, markdown, or vancouver-like bracket).",
                json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "Target text where citation should be inserted." },
                        "citation_key": { "type": "string", "description": "Primary citation key to insert." },
                        "citation_keys": { "type": "array", "items": { "type": "string" }, "description": "Optional fallback citation keys; first non-empty key is used." },
                        "style": { "type": "string", "description": "Optional style: latex | markdown | vancouver." },
                        "placement": { "type": "string", "description": "Optional placement mode: sentence_end | append." },
                        "dedupe": { "type": "boolean", "description": "If true, avoid inserting duplicate markers already present in text." }
                    },
                    "required": ["text"],
                    "additionalProperties": false
                }),
            ),
            make_tool_spec(
                "review_manuscript",
                "Perform a structured peer-review scan on manuscript text or file/document path and return severity-tagged findings.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Optional project-relative manuscript path (supports PDF/DOCX via document runtime)." },
                        "text": { "type": "string", "description": "Optional inline manuscript text if path is not provided." },
                        "focus": { "type": "string", "description": "Optional review focus (e.g., novelty, methods rigor, clarity)." },
                        "checklist": {
                            "description": "Optional checklist tags such as CONSORT/PRISMA/STROBE.",
                            "oneOf": [
                                { "type": "array", "items": { "type": "string" } },
                                { "type": "string" }
                            ]
                        }
                    },
                    "additionalProperties": false
                }),
            ),
            make_tool_spec(
                "check_statistics",
                "Check statistical reporting quality in manuscript text and flag unsupported significance claims or missing uncertainty reporting.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Optional project-relative manuscript path." },
                        "text": { "type": "string", "description": "Optional inline manuscript text if path is not provided." }
                    },
                    "additionalProperties": false
                }),
            ),
            make_tool_spec(
                "verify_references",
                "Verify internal citation-style consistency and detect potentially uncited narrative claims.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Optional project-relative manuscript path." },
                        "text": { "type": "string", "description": "Optional inline manuscript text if path is not provided." }
                    },
                    "additionalProperties": false
                }),
            ),
            make_tool_spec(
                "generate_response_letter",
                "Generate a point-by-point response letter draft from reviewer comments and optional revision plan notes.",
                json!({
                    "type": "object",
                    "properties": {
                        "reviewer_comments": {
                            "description": "Reviewer comments as an array or newline-delimited string.",
                            "oneOf": [
                                { "type": "array", "items": { "type": "string" } },
                                { "type": "string" }
                            ]
                        },
                        "revision_plan": { "type": "string", "description": "Optional high-level revision summary." },
                        "tone": { "type": "string", "description": "Optional response tone (default professional)." }
                    },
                    "required": ["reviewer_comments"],
                    "additionalProperties": false
                }),
            ),
            make_tool_spec(
                "track_revisions",
                "Track revision delta between old and new text (inline or file paths) and summarize changed line/word counts.",
                json!({
                    "type": "object",
                    "properties": {
                        "old_text": { "type": "string", "description": "Old manuscript text." },
                        "new_text": { "type": "string", "description": "New manuscript text." },
                        "old_path": { "type": "string", "description": "Path to old version file if old_text is omitted." },
                        "new_path": { "type": "string", "description": "Path to new version file if new_text is omitted." }
                    },
                    "additionalProperties": false
                }),
            ),
        ]);
    }

    specs.extend([
        make_tool_spec(
            "replace_selected_text",
            "Replace the currently selected text in a project file without rewriting the rest of the file. Use this for selection-scoped edits. REQUIREMENT: expected_selected_text must match the selected file content exactly, including whitespace and line breaks. If a selection_anchor is present, pass it exactly as provided in the prompt context. Do not use this tool for multi-location or whole-file rewrites.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project-relative file path." },
                    "expected_selected_text": { "type": "string", "description": "Exact text expected to be selected in the file." },
                    "replacement_text": { "type": "string", "description": "Replacement text for the selected span." },
                    "selection_anchor": { "type": "string", "description": "Optional selection anchor like @path:startLine:startCol-endLine:endCol." }
                },
                "required": ["path", "expected_selected_text", "replacement_text"],
                "additionalProperties": false
            }),
        ),
        make_tool_spec(
            "apply_text_patch",
            "Apply a precise text patch to a file. REQUIREMENT: expected_old_text must match the file content character-for-character, including spaces, indentation, and newlines. BEFORE CALLING: use read_file to retrieve the exact current content. DO NOT paraphrase, shorten, or reformat expected_old_text. If the text appears in multiple places, include more surrounding lines so the match is unique. Use write_file only for whole-file rewrites or new files.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project-relative file path." },
                    "expected_old_text": { "type": "string", "description": "Exact text that must already exist in the file." },
                    "new_text": { "type": "string", "description": "Replacement text." }
                },
                "required": ["path", "expected_old_text", "new_text"],
                "additionalProperties": false
            }),
        ),
        make_tool_spec(
            "write_file",
            "Write full replacement content to a file in the current project. Use this only for whole-file rewrites, creating a new file, or final apply steps after review. Do not use write_file for selection-scoped edits or narrow paragraph patches.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project-relative file path." },
                    "content": { "type": "string", "description": "Full replacement file content." }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        ),
        make_tool_spec(
            "list_files",
            "List files inside the current project.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Optional subdirectory inside the project." }
                },
                "additionalProperties": false
            }),
        ),
        make_tool_spec(
            "search_project",
            "Search for text in the current project using ripgrep.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Text or regex query." },
                    "path": { "type": "string", "description": "Optional subdirectory inside the project." }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        make_tool_spec(
            "run_shell_command",
            "Run a shell command in the current project. Use this for explicit engineering or environment tasks, not for default document reading or attachment extraction.",
            json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to run." }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
        ),
    ]);

    specs
}

pub fn to_openai_tool_schema(spec: &AgentToolSpec) -> Value {
    let _ = spec.contract;
    json!({
        "type": "function",
        "name": spec.name,
        "description": spec.description,
        "parameters": adapt_tool_input_schema_for_provider(&spec.input_schema, "openai"),
    })
}

pub fn to_chat_completions_tool_schema(spec: &AgentToolSpec, provider: &str) -> Value {
    let _ = spec.contract;
    json!({
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": adapt_tool_input_schema_for_provider(&spec.input_schema, provider),
        }
    })
}

fn adapt_tool_input_schema_for_provider(schema: &Value, provider: &str) -> Value {
    let mut adapted = schema.clone();
    if matches!(provider, "minimax" | "deepseek") {
        strip_additional_properties_false(&mut adapted);
    }
    adapted
}

fn strip_additional_properties_false(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.get("additionalProperties") == Some(&Value::Bool(false)) {
                map.remove("additionalProperties");
            }
            for nested in map.values_mut() {
                strip_additional_properties_false(nested);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_additional_properties_false(item);
            }
        }
        _ => {}
    }
}

pub async fn execute_tool_call(
    runtime_state: &AgentRuntimeState,
    tab_id: &str,
    project_root: &str,
    call: AgentToolCall,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> AgentToolResult {
    if is_cancelled(cancel_rx.as_ref()) {
        return error_result(
            &call.tool_name,
            &call.call_id,
            AGENT_CANCELLED_MESSAGE.to_string(),
        );
    }

    let parsed_args = match serde_json::from_str::<Value>(&call.arguments) {
        Ok(value) => value,
        Err(err) => {
            return error_result(
                &call.tool_name,
                &call.call_id,
                format!("Invalid tool arguments JSON: {}", err),
            )
        }
    };

    match call.tool_name.as_str() {
        "read_file" => execute_read_file(project_root, &call.call_id, parsed_args, cancel_rx).await,
        "read_document" => {
            execute_read_document(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "search_literature" => {
            execute_search_literature(&call.call_id, parsed_args, cancel_rx).await
        }
        "analyze_paper" => {
            execute_analyze_paper(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "compare_papers" => {
            execute_compare_papers(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "synthesize_evidence" => {
            execute_synthesize_evidence(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "extract_methodology" => {
            execute_extract_methodology(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "inspect_resource" => {
            execute_inspect_resource(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "read_document_excerpt" => {
            execute_read_document_excerpt(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "search_document_text" => {
            execute_search_document_text(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "get_document_evidence" => {
            execute_get_document_evidence(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "draft_section" => execute_draft_section(&call.call_id, parsed_args, cancel_rx).await,
        "restructure_outline" => {
            execute_restructure_outline(&call.call_id, parsed_args, cancel_rx).await
        }
        "check_consistency" => {
            execute_check_consistency(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "generate_abstract" => {
            execute_generate_abstract(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "insert_citation" => execute_insert_citation(&call.call_id, parsed_args, cancel_rx).await,
        "review_manuscript" => {
            execute_review_manuscript(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "check_statistics" => {
            execute_check_statistics(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "verify_references" => {
            execute_verify_references(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "generate_response_letter" => {
            execute_generate_response_letter(&call.call_id, parsed_args, cancel_rx).await
        }
        "track_revisions" => {
            execute_track_revisions(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "replace_selected_text" => {
            execute_replace_selected_text(
                runtime_state,
                tab_id,
                project_root,
                &call.call_id,
                parsed_args,
                cancel_rx,
            )
            .await
        }
        "apply_text_patch" => {
            execute_apply_text_patch(
                runtime_state,
                tab_id,
                project_root,
                &call.call_id,
                parsed_args,
                cancel_rx,
            )
            .await
        }
        "write_file" => {
            execute_write_file(
                runtime_state,
                tab_id,
                project_root,
                &call.call_id,
                parsed_args,
                cancel_rx,
            )
            .await
        }
        "list_files" => {
            execute_list_files(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "search_project" => {
            execute_search_project(project_root, &call.call_id, parsed_args, cancel_rx).await
        }
        "run_shell_command" => {
            execute_run_shell_command(
                runtime_state,
                tab_id,
                project_root,
                &call.call_id,
                parsed_args,
                cancel_rx,
            )
            .await
        }
        other => error_result(
            other,
            &call.call_id,
            format!("Unknown local tool: {}", other),
        ),
    }
}

fn error_result(tool_name: &str, call_id: &str, message: String) -> AgentToolResult {
    AgentToolResult {
        tool_name: tool_name.to_string(),
        call_id: call_id.to_string(),
        is_error: true,
        preview: truncate_preview(&message),
        content: json!({ "error": message }),
    }
}

pub fn summarize_tool_target(content: &Value) -> Option<String> {
    [
        "path",
        "file_path",
        "targetPath",
        "filePath",
        "command",
        "query",
    ]
    .iter()
    .find_map(|key| content.get(*key).and_then(Value::as_str))
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(str::to_string)
}

pub fn tool_result_requires_approval(result: &AgentToolResult) -> bool {
    result
        .content
        .get("approvalRequired")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn tool_result_review_ready(result: &AgentToolResult) -> bool {
    result
        .content
        .get("reviewArtifact")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn tool_result_display_content(result: &AgentToolResult) -> AgentToolResultDisplayContent {
    let review_artifact_payload = result
        .content
        .get("reviewArtifactPayload")
        .and_then(Value::as_object);
    let target_path = summarize_tool_target(&result.content).or_else(|| {
        review_artifact_payload.and_then(|payload| {
            payload
                .get("targetPath")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
    });
    let command = result
        .content
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            result
                .content
                .get("input")
                .and_then(Value::as_object)
                .and_then(|payload| payload.get("command"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let query = result
        .content
        .get("query")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            result
                .content
                .get("input")
                .and_then(Value::as_object)
                .and_then(|payload| payload.get("query"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let approval_required = tool_result_requires_approval(result);
    let review_ready = tool_result_review_ready(result);
    let approval_reason = result
        .content
        .get("reason")
        .and_then(Value::as_str)
        .map(str::to_string);
    let approval_tool_name = result
        .content
        .get("approvalToolName")
        .and_then(Value::as_str)
        .or_else(|| result.content.get("toolName").and_then(Value::as_str))
        .map(str::to_string);
    let summary = review_artifact_payload
        .and_then(|payload| payload.get("summary"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            result
                .content
                .get("summary")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let content_text = result
        .content
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_string);
    let error_text = result
        .content
        .get("error")
        .and_then(Value::as_str)
        .map(str::to_string);
    let status = if approval_required && review_ready {
        "review_ready"
    } else if approval_required {
        "awaiting_approval"
    } else if result.is_error {
        "error"
    } else {
        "completed"
    };

    AgentToolResultDisplayContent {
        kind: "tool_result_display".to_string(),
        display_kind: tool_display_kind(&result.tool_name).to_string(),
        tool_name: result.tool_name.clone(),
        status: status.to_string(),
        text_preview: error_text
            .or(content_text)
            .or(approval_reason.clone())
            .or(summary.clone())
            .unwrap_or_else(|| result.preview.clone()),
        is_error: result.is_error,
        target_path,
        command,
        query,
        approval_required,
        review_ready,
        approval_reason,
        approval_tool_name,
        written: result.content.get("written").and_then(Value::as_bool),
        summary,
    }
}

pub fn tool_result_display_value(result: &AgentToolResult) -> Value {
    serde_json::to_value(tool_result_display_content(result)).unwrap_or_else(|_| {
        json!({
            "kind": "tool_result_display",
            "displayKind": tool_display_kind(&result.tool_name),
            "toolName": result.tool_name,
            "status": if result.is_error { "error" } else { "completed" },
            "textPreview": result.preview,
            "isError": result.is_error,
            "approvalRequired": tool_result_requires_approval(result),
            "reviewReady": tool_result_review_ready(result),
        })
    })
}

pub fn check_tool_call_policy(
    context: ToolExecutionPolicyContext,
    call: &AgentToolCall,
    target: Option<&str>,
) -> Option<AgentToolResult> {
    if context.task_kind == AgentTaskKind::SelectionEdit && call.tool_name == "write_file" {
        return Some(AgentToolResult {
            tool_name: call.tool_name.clone(),
            call_id: call.call_id.clone(),
            is_error: true,
            preview: "Selection-scoped edits must not use write_file.".to_string(),
            content: json!({
                "error": "selection-scoped edits must not use write_file",
                "disallowedByPolicy": true,
                "attemptedTool": "write_file",
                "suggestedTools": ["replace_selected_text", "apply_text_patch"]
            }),
        });
    }

    if matches!(
        context.task_kind,
        AgentTaskKind::Analysis | AgentTaskKind::LiteratureReview | AgentTaskKind::PeerReview
    ) && context.has_binary_attachment_context
        && call.tool_name == "run_shell_command"
    {
        return Some(AgentToolResult {
            tool_name: call.tool_name.clone(),
            call_id: call.call_id.clone(),
            is_error: true,
            preview: "Attachment-backed PDF/DOCX analysis must not use shell probing.".to_string(),
            content: json!({
                "error": "attachment-backed PDF/DOCX analysis must not use run_shell_command for exploratory extraction",
                "disallowedByPolicy": true,
                "attemptedTool": "run_shell_command",
                "suggestedAction": "Use read_document instead."
            }),
        });
    }

    if matches!(
        context.task_kind,
        AgentTaskKind::Analysis | AgentTaskKind::LiteratureReview | AgentTaskKind::PeerReview
    ) && context.has_binary_attachment_context
        && call.tool_name == "read_file"
        && target.map(is_document_resource_path).unwrap_or(false)
    {
        return Some(AgentToolResult {
            tool_name: call.tool_name.clone(),
            call_id: call.call_id.clone(),
            is_error: true,
            preview:
                "Binary attachment analysis must rely on ingested excerpts, not raw file reads."
                    .to_string(),
            content: json!({
                "error": "attachment-backed PDF/DOCX analysis must not use read_file on binary resources",
                "disallowedByPolicy": true,
                "attemptedTool": "read_file",
                "suggestedAction": "Use read_document instead."
            }),
        });
    }

    None
}

fn ok_result(tool_name: &str, call_id: &str, content: Value, preview: String) -> AgentToolResult {
    AgentToolResult {
        tool_name: tool_name.to_string(),
        call_id: call_id.to_string(),
        is_error: false,
        content,
        preview: truncate_preview(&preview),
    }
}

fn approval_required_result(
    tool_name: &str,
    call_id: &str,
    reason: String,
    args: Value,
) -> AgentToolResult {
    AgentToolResult {
        tool_name: tool_name.to_string(),
        call_id: call_id.to_string(),
        is_error: true,
        preview: truncate_preview(&reason),
        content: json!({
            "approvalRequired": true,
            "toolName": tool_name,
            "reason": reason,
            "input": args,
        }),
    }
}

fn build_review_artifact(
    tool_name: &str,
    approval_tool_name: &str,
    raw_path: &str,
    full_path: &Path,
    old_content: &str,
    new_content: &str,
    written: bool,
    extra: &Value,
) -> AgentReviewArtifact {
    let selection_range = extra
        .get("selectionAnchor")
        .and_then(Value::as_str)
        .map(str::to_string);
    let summary = extra
        .get("summary")
        .and_then(Value::as_str)
        .map(str::to_string);

    AgentReviewArtifact {
        artifact_type: "text_edit".to_string(),
        tool_name: tool_name.to_string(),
        approval_tool_name: approval_tool_name.to_string(),
        target_path: raw_path.to_string(),
        absolute_path: full_path.to_string_lossy().to_string(),
        old_content: old_content.to_string(),
        new_content: new_content.to_string(),
        selection_range,
        summary,
        written,
    }
}

fn approval_required_edit_result(
    tool_name: &str,
    approval_tool_name: &str,
    call_id: &str,
    reason: &str,
    args: Value,
    raw_path: &str,
    full_path: &Path,
    old_content: &str,
    new_content: &str,
    extra: Value,
) -> AgentToolResult {
    let artifact = build_review_artifact(
        tool_name,
        approval_tool_name,
        raw_path,
        full_path,
        old_content,
        new_content,
        false,
        &extra,
    );
    let content = artifact.to_content_value(true, Some(reason), Some(args), extra);

    AgentToolResult {
        tool_name: tool_name.to_string(),
        call_id: call_id.to_string(),
        is_error: true,
        preview: truncate_preview(reason),
        content,
    }
}

fn truncate_preview(value: &str) -> String {
    if value.chars().count() > MAX_PREVIEW_CHARS {
        format!(
            "{}...",
            value.chars().take(MAX_PREVIEW_CHARS).collect::<String>()
        )
    } else {
        value.to_string()
    }
}

fn truncate_at_boundary(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.trim().to_string();
    }

    let slice = value.chars().take(max_chars).collect::<String>();
    let last_break = slice
        .rfind('\n')
        .or_else(|| slice.rfind(' '))
        .unwrap_or(slice.len());
    let trimmed = if last_break > max_chars / 2 {
        &slice[..last_break]
    } else {
        &slice
    };
    trimmed.trim().to_string()
}

fn split_fallback_document_segments(text: &str) -> Vec<DocumentArtifactSegment> {
    let normalized = text.replace("\r\n", "\n");
    let pages = normalized
        .split('\u{000C}')
        .map(str::trim)
        .filter(|page| !page.is_empty())
        .collect::<Vec<_>>();

    if pages.len() > 1 {
        return pages
            .into_iter()
            .enumerate()
            .map(|(index, text)| DocumentArtifactSegment {
                label: format!("Page {} (fallback)", index + 1),
                text: text.to_string(),
            })
            .collect();
    }

    normalized
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .enumerate()
        .map(|(index, text)| DocumentArtifactSegment {
            label: format!("Paragraph {} (fallback)", index + 1),
            text: text.to_string(),
        })
        .collect()
}

fn document_runtime_from_artifact(artifact: DocumentArtifact) -> DocumentRuntimeContent {
    DocumentRuntimeContent {
        kind: artifact.kind,
        source_type: artifact.source_type,
        extraction_status: artifact.extraction_status,
        excerpt: artifact.excerpt,
        searchable_text: artifact.searchable_text,
        segments: artifact.segments,
        page_count: artifact.page_count,
        fallback_used: false,
    }
}

async fn try_pdf_document_shell_fallback(
    project_root: &str,
    raw_path: &str,
    full_path: &Path,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<Option<DocumentRuntimeContent>, String> {
    if resource_kind_from_path(raw_path) != "pdf_document" {
        return Ok(None);
    }

    if !process_utils::command_available("pdftotext", project_root).await {
        return Ok(None);
    }

    let file_arg = full_path.to_string_lossy().to_string();
    let attempts = vec![
        ("default", vec![file_arg.clone(), "-".to_string()]),
        (
            "layout",
            vec!["-layout".to_string(), file_arg.clone(), "-".to_string()],
        ),
    ];
    let mut last_error: Option<String> = None;
    let mut fallback_mode = "default";
    let mut searchable_text = String::new();

    for (mode, args) in attempts {
        let result = process_utils::run_program_with_limits(
            "pdftotext",
            args,
            project_root.to_string(),
            cancel_rx.clone(),
            DOCUMENT_FALLBACK_TIMEOUT_SECS,
            DOCUMENT_FALLBACK_MAX_BYTES,
        )
        .await?;

        if result.exit_code != 0 {
            let stderr = result.stderr.trim();
            last_error = Some(if stderr.is_empty() {
                format!("pdftotext ({}) failed while extracting {}", mode, raw_path)
            } else {
                format!(
                    "pdftotext ({}) failed while extracting {}: {}",
                    mode, raw_path, stderr
                )
            });
            continue;
        }

        let output = result.stdout.trim();
        if output.is_empty() {
            continue;
        }

        fallback_mode = mode;
        searchable_text = output.to_string();
        break;
    }

    if searchable_text.is_empty() {
        if let Some(message) = last_error {
            return Err(message);
        }
        return Ok(None);
    }

    let segments = split_fallback_document_segments(&searchable_text);
    let excerpt_body = truncate_at_boundary(&searchable_text, DOCUMENT_EXCERPT_MAX_CHARS);
    let excerpt = if excerpt_body.is_empty() {
        format!(
            "[Attached PDF fallback excerpt from {}]\nNo extractable text was found.",
            raw_path
        )
    } else {
        format!(
            "[Attached PDF fallback excerpt from {}]\n{}",
            raw_path, excerpt_body
        )
    };

    Ok(Some(DocumentRuntimeContent {
        kind: "pdf_document".to_string(),
        source_type: "pdf".to_string(),
        extraction_status: format!("fallback_shell_{}", fallback_mode),
        excerpt,
        searchable_text,
        page_count: None,
        segments,
        fallback_used: true,
    }))
}

async fn load_document_runtime_content(
    project_root: &str,
    raw_path: &str,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<DocumentRuntimeContent, String> {
    match load_document_artifact(project_root, raw_path).await {
        Ok(artifact)
            if artifact.extraction_status != "image_only"
                && artifact.extraction_status != "failed" =>
        {
            Ok(document_runtime_from_artifact(artifact))
        }
        Ok(artifact) => {
            let full_path = Path::new(project_root).join(ensure_relative_path(raw_path)?);
            if let Some(fallback) =
                try_pdf_document_shell_fallback(project_root, raw_path, &full_path, cancel_rx)
                    .await?
            {
                Ok(fallback)
            } else {
                Ok(document_runtime_from_artifact(artifact))
            }
        }
        Err(load_error) => {
            let full_path = Path::new(project_root).join(ensure_relative_path(raw_path)?);
            if let Some(fallback) =
                try_pdf_document_shell_fallback(project_root, raw_path, &full_path, cancel_rx)
                    .await?
            {
                Ok(fallback)
            } else {
                Err(load_error)
            }
        }
    }
}

fn truncate_file_bytes(bytes: &[u8]) -> (&[u8], bool) {
    if bytes.len() <= MAX_FILE_BYTES {
        return (bytes, false);
    }

    let mut end = MAX_FILE_BYTES.min(bytes.len());
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }

    if let Some(last_newline) = bytes[..end].iter().rposition(|byte| *byte == b'\n') {
        if last_newline > 0 {
            end = last_newline + 1;
        }
    }

    (&bytes[..end], true)
}

async fn ensure_ripgrep_available(
    tool_name: &str,
    call_id: &str,
    project_root: &str,
) -> Result<(), AgentToolResult> {
    if process_utils::command_available("rg", project_root).await {
        Ok(())
    } else {
        Err(error_result(
            tool_name,
            call_id,
            "ripgrep (`rg`) is not available on this machine. Install `rg` or use read_file on a known path instead.".to_string(),
        ))
    }
}

fn is_cancelled(cancel_rx: Option<&watch::Receiver<bool>>) -> bool {
    cancel_rx.map(|rx| *rx.borrow()).unwrap_or(false)
}

fn cancelled_result(tool_name: &str, call_id: &str) -> AgentToolResult {
    error_result(tool_name, call_id, AGENT_CANCELLED_MESSAGE.to_string())
}

async fn command_output_with_cancel(
    command: Command,
    cancel_rx: Option<watch::Receiver<bool>>,
    tool_name: &str,
    call_id: &str,
    spawn_error_prefix: &str,
) -> Result<std::process::Output, AgentToolResult> {
    if is_cancelled(cancel_rx.as_ref()) {
        return Err(cancelled_result(tool_name, call_id));
    }

    process_utils::wait_for_command_output(command, cancel_rx)
        .await
        .map_err(|message| {
            if message == AGENT_CANCELLED_MESSAGE {
                cancelled_result(tool_name, call_id)
            } else {
                error_result(
                    tool_name,
                    call_id,
                    format!("{}: {}", spawn_error_prefix, message),
                )
            }
        })
}

fn ensure_relative_path(raw_path: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(raw_path);
    if candidate.is_absolute() {
        return Err("absolute paths are not allowed".to_string());
    }
    if candidate.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err("path traversal is not allowed".to_string());
    }
    Ok(candidate.to_path_buf())
}

fn resolve_project_path(project_root: &str, raw_path: &str) -> Result<PathBuf, String> {
    let relative = ensure_relative_path(raw_path)?;
    Ok(Path::new(project_root).join(relative))
}

fn tool_arg_string(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("missing required string argument: {}", key))
}

fn tool_arg_optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn tool_arg_optional_usize(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn parse_selection_anchor(raw: &str) -> Result<SelectionAnchor, String> {
    let trimmed = raw.trim().trim_start_matches('@');
    let (path, rest) = trimmed
        .split_once(':')
        .ok_or_else(|| "selection_anchor is missing file path".to_string())?;
    let (start, end) = rest
        .split_once('-')
        .ok_or_else(|| "selection_anchor range must contain '-'".to_string())?;
    let (start_line, start_col) = start
        .split_once(':')
        .ok_or_else(|| "selection_anchor start must be line:col".to_string())?;
    let (end_line, end_col) = end
        .split_once(':')
        .ok_or_else(|| "selection_anchor end must be line:col".to_string())?;

    let parse_part = |value: &str, label: &str| {
        value
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("selection_anchor {} is not a valid integer", label))
    };

    Ok(SelectionAnchor {
        path: path.to_string(),
        start_line: parse_part(start_line, "start line")?,
        start_col: parse_part(start_col, "start col")?,
        end_line: parse_part(end_line, "end line")?,
        end_col: parse_part(end_col, "end col")?,
    })
}

fn line_col_to_byte_offset(content: &str, target_line: usize, target_col: usize) -> Option<usize> {
    if target_line == 0 || target_col == 0 {
        return None;
    }

    let mut line = 1usize;
    let mut col = 1usize;

    if target_line == 1 && target_col == 1 {
        return Some(0);
    }

    for (idx, ch) in content.char_indices() {
        if line == target_line && col == target_col {
            return Some(idx);
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }

    if line == target_line && col == target_col {
        Some(content.len())
    } else {
        None
    }
}

fn replace_byte_range(content: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut updated = String::with_capacity(content.len() - (end - start) + replacement.len());
    updated.push_str(&content[..start]);
    updated.push_str(replacement);
    updated.push_str(&content[end..]);
    updated
}

fn find_occurrence_offsets(content: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut search_from = 0usize;
    while let Some(relative) = content[search_from..].find(needle) {
        let start = search_from + relative;
        let end = start + needle.len();
        matches.push((start, end));
        search_from = end;
    }
    matches
}

fn replace_unique_exact(
    content: &str,
    expected: &str,
    replacement: &str,
) -> Result<String, String> {
    if expected.is_empty() {
        return Err("expected text must not be empty".to_string());
    }

    let matches = find_occurrence_offsets(content, expected);
    match matches.as_slice() {
        [] => {
            let needle_hint = expected
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim();
            let nearest_line = if needle_hint.is_empty() {
                None
            } else {
                content
                    .lines()
                    .enumerate()
                    .find(|(_, line)| line.contains(needle_hint))
                    .map(|(index, line)| {
                        let snippet = line.chars().take(120).collect::<String>();
                        format!(" Nearest line {}: {:?}", index + 1, snippet)
                    })
            };
            Err(format!(
                "expected_old_text was not found verbatim in the file (checked {} chars).{} Make sure the text matches exactly, including whitespace and line breaks. Call read_file first to get the current content.",
                content.len(),
                nearest_line.unwrap_or_default()
            ))
        }
        [(start, end)] => Ok(replace_byte_range(content, *start, *end, replacement)),
        _ => Err(format!(
            "expected_old_text matched {} locations; retry with a longer, more specific exact excerpt that uniquely identifies the target location.",
            matches.len()
        )),
    }
}

fn replace_unique_with_trimmed_fallback(
    content: &str,
    expected: &str,
    replacement: &str,
) -> Result<(String, bool), String> {
    match replace_unique_exact(content, expected, replacement) {
        Ok(updated) => Ok((updated, false)),
        Err(primary_error) => {
            let trimmed_expected = expected.trim();
            if trimmed_expected.is_empty() || trimmed_expected == expected {
                return Err(primary_error);
            }

            match replace_unique_exact(content, trimmed_expected, replacement) {
                Ok(updated) => Ok((updated, true)),
                Err(trimmed_error) => Err(format!(
                    "{} A conservative trimmed retry also failed: {}",
                    primary_error, trimmed_error
                )),
            }
        }
    }
}

fn replace_by_anchor(
    raw_path: &str,
    content: &str,
    expected_selected_text: &str,
    replacement_text: &str,
    selection_anchor: &str,
) -> Result<Option<String>, String> {
    let anchor = parse_selection_anchor(selection_anchor)?;
    if anchor.path != raw_path {
        return Err(format!(
            "selection_anchor path {} does not match target path {}",
            anchor.path, raw_path
        ));
    }

    let start = line_col_to_byte_offset(content, anchor.start_line, anchor.start_col)
        .ok_or_else(|| "selection_anchor start is outside the file".to_string())?;
    let end = line_col_to_byte_offset(content, anchor.end_line, anchor.end_col)
        .ok_or_else(|| "selection_anchor end is outside the file".to_string())?;

    if start > end || end > content.len() {
        return Err("selection_anchor resolves to an invalid range".to_string());
    }

    let selected = &content[start..end];
    if selected == expected_selected_text {
        return Ok(Some(replace_byte_range(
            content,
            start,
            end,
            replacement_text,
        )));
    }

    Ok(None)
}

async fn read_existing_file_for_edit(
    tool_name: &str,
    call_id: &str,
    raw_path: &str,
    full_path: &Path,
    cancel_rx: Option<&watch::Receiver<bool>>,
) -> Result<String, AgentToolResult> {
    if is_cancelled(cancel_rx) {
        return Err(cancelled_result(tool_name, call_id));
    }
    match tokio::fs::read_to_string(full_path).await {
        Ok(existing) => Ok(existing),
        Err(err) => Err(error_result(
            tool_name,
            call_id,
            format!("Failed to read existing {} before edit: {}", raw_path, err),
        )),
    }
}

fn files_preview(prefix: &str, lines: &[String], truncated: bool) -> String {
    let mut preview = format!("{}:\n{}", prefix, lines.join("\n"));
    if truncated {
        preview.push_str("\n...[truncated]");
    }
    preview
}

#[cfg(test)]
mod tests {
    use super::{
        build_default_tool_specs, default_tool_specs, ensure_relative_path,
        execute_apply_text_patch, execute_read_document_excerpt, execute_read_file,
        execute_replace_selected_text, line_col_to_byte_offset, parse_selection_anchor,
        replace_by_anchor, replace_unique_exact, replace_unique_with_trimmed_fallback,
        to_openai_tool_schema, tool_contract, truncate_file_bytes, AgentToolSpec, MAX_FILE_BYTES,
    };
    use crate::agent::document_artifacts::artifact_path_for;
    use crate::agent::session::AgentRuntimeState;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn rejects_parent_dir_traversal() {
        assert!(ensure_relative_path("../secrets.txt").is_err());
        assert!(ensure_relative_path("/tmp/file").is_err());
        assert!(ensure_relative_path("safe/path.tex").is_ok());
    }

    #[test]
    fn parses_selection_anchor() {
        let anchor = parse_selection_anchor("@main.tex:14:1-14:20").unwrap();
        assert_eq!(anchor.path, "main.tex");
        assert_eq!(anchor.start_line, 14);
        assert_eq!(anchor.start_col, 1);
        assert_eq!(anchor.end_line, 14);
        assert_eq!(anchor.end_col, 20);
    }

    #[test]
    fn resolves_line_col_to_byte_offset() {
        let content = "abc\ndef\n";
        assert_eq!(line_col_to_byte_offset(content, 1, 1), Some(0));
        assert_eq!(line_col_to_byte_offset(content, 2, 1), Some(4));
        assert_eq!(line_col_to_byte_offset(content, 2, 4), Some(7));
    }

    #[test]
    fn replace_unique_exact_replaces_only_match() {
        let updated = replace_unique_exact("alpha beta gamma", "beta", "BETA").unwrap();
        assert_eq!(updated, "alpha BETA gamma");
    }

    #[test]
    fn replace_unique_exact_rejects_ambiguous_match() {
        let err = replace_unique_exact("x x x", "x", "y").unwrap_err();
        assert!(err.contains("matched 3 locations"));
    }

    #[test]
    fn replace_by_anchor_prefers_targeted_range() {
        let content = "first line\nsecond line\nthird line";
        let updated = replace_by_anchor(
            "main.tex",
            content,
            "second line",
            "SECOND LINE",
            "@main.tex:2:1-2:12",
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated, "first line\nSECOND LINE\nthird line");
    }

    #[test]
    fn maps_tool_spec_to_openai_function_shape() {
        let tool = AgentToolSpec {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: json!({"type": "object"}),
            contract: tool_contract("read_file"),
        };
        let mapped = to_openai_tool_schema(&tool);
        assert_eq!(mapped["type"], "function");
        assert_eq!(mapped["name"], "read_file");
        assert_eq!(mapped["description"], "Read a file");
        assert_eq!(mapped["parameters"]["type"], "object");
    }

    #[test]
    fn exposes_single_document_tool_entry_in_default_specs() {
        let names = default_tool_specs()
            .into_iter()
            .map(|spec| spec.name)
            .collect::<Vec<_>>();

        assert!(names.iter().any(|name| name == "read_document"));
        assert!(!names.iter().any(|name| name == "inspect_resource"));
        assert!(!names.iter().any(|name| name == "read_document_excerpt"));
        assert!(!names.iter().any(|name| name == "search_document_text"));
        assert!(!names.iter().any(|name| name == "get_document_evidence"));
    }

    #[test]
    fn includes_writing_tools_when_enabled() {
        let names = build_default_tool_specs(true)
            .into_iter()
            .map(|spec| spec.name)
            .collect::<Vec<_>>();

        assert!(names.iter().any(|name| name == "draft_section"));
        assert!(names.iter().any(|name| name == "restructure_outline"));
        assert!(names.iter().any(|name| name == "check_consistency"));
        assert!(names.iter().any(|name| name == "generate_abstract"));
        assert!(names.iter().any(|name| name == "insert_citation"));
    }

    #[test]
    fn excludes_writing_tools_when_disabled() {
        let names = build_default_tool_specs(false)
            .into_iter()
            .map(|spec| spec.name)
            .collect::<Vec<_>>();

        assert!(!names.iter().any(|name| name == "draft_section"));
        assert!(!names.iter().any(|name| name == "restructure_outline"));
        assert!(!names.iter().any(|name| name == "check_consistency"));
        assert!(!names.iter().any(|name| name == "generate_abstract"));
        assert!(!names.iter().any(|name| name == "insert_citation"));
    }

    #[test]
    fn truncate_file_bytes_keeps_utf8_intact() {
        let content = format!("第一行\n{}\n", "中".repeat(MAX_FILE_BYTES));
        let (slice, truncated) = truncate_file_bytes(content.as_bytes());
        let text = String::from_utf8(slice.to_vec()).unwrap();

        assert!(truncated);
        assert!(!text.contains('\u{fffd}'));
    }

    #[tokio::test]
    async fn read_file_rejects_document_resources() {
        let temp = tempdir().unwrap();
        let project_root = temp.path().to_string_lossy().to_string();
        let file_path = temp.path().join("paper.pdf");
        tokio::fs::write(&file_path, b"%PDF-1.4").await.unwrap();

        let result = execute_read_file(
            &project_root,
            "call-doc-read",
            json!({ "path": "paper.pdf" }),
            None,
        )
        .await;

        assert!(result.is_error);
        assert!(result.content["error"]
            .as_str()
            .unwrap_or_default()
            .contains("document resource"));
    }

    #[tokio::test]
    async fn read_document_excerpt_uses_persisted_artifact() {
        let temp = tempdir().unwrap();
        let project_root = temp.path().to_string_lossy().to_string();
        let file_path = temp.path().join("attachments").join("paper.pdf");
        tokio::fs::create_dir_all(file_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&file_path, b"%PDF-1.4").await.unwrap();

        let artifact_path = artifact_path_for(&project_root, "attachments/paper.pdf");
        tokio::fs::create_dir_all(artifact_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(
            &artifact_path,
            serde_json::to_string_pretty(&json!({
                "version": 2,
                "filePath": "attachments/paper.pdf",
                "absolutePath": file_path.to_string_lossy(),
                "sourceType": "pdf",
                "kind": "pdf_document",
                "extractionStatus": "ready",
                "excerpt": "[Attached PDF excerpt from attachments/paper.pdf]\nPage 2:\nhydrophobic surface treatment",
                "searchableText": "hydrophobic surface treatment",
                "segments": [
                    {
                        "label": "Page 2",
                        "text": "hydrophobic surface treatment"
                    }
                ],
                "pageCount": 3,
                "metadata": {}
            }))
            .unwrap(),
        )
        .await
        .unwrap();

        let result = execute_read_document_excerpt(
            &project_root,
            "call-doc-excerpt",
            json!({ "path": "attachments/paper.pdf" }),
            None,
        )
        .await;

        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(result.content["path"], json!("attachments/paper.pdf"));
        assert!(result.preview.contains("hydrophobic surface treatment"));
    }

    #[test]
    fn trim_fallback_can_recover_exact_patch() {
        let (updated, used_trimmed_fallback) = replace_unique_with_trimmed_fallback(
            "before\nline to patch\nafter\n",
            "  line to patch\n",
            "patched line",
        )
        .unwrap();

        assert!(used_trimmed_fallback);
        assert_eq!(updated, "before\npatched line\nafter\n");
    }

    #[tokio::test]
    async fn replace_selected_text_updates_only_the_targeted_selection() {
        let temp = tempdir().unwrap();
        let project_root = temp.path().to_string_lossy().to_string();
        let file_path = temp.path().join("main.tex");
        let original = [
            "Introduction paragraph.",
            "",
            "TARGET paragraph to refine.",
            "",
            "TARGET paragraph to refine.",
            "",
            "Closing paragraph.",
        ]
        .join("\n");
        tokio::fs::write(&file_path, original.as_bytes())
            .await
            .unwrap();

        let runtime_state = AgentRuntimeState::default();
        runtime_state
            .set_tool_approval("tab-selection", "patch_file", "allow_session")
            .await
            .unwrap();

        let result = execute_replace_selected_text(
            &runtime_state,
            "tab-selection",
            &project_root,
            "call-1",
            json!({
                "path": "main.tex",
                "expected_selected_text": "TARGET paragraph to refine.",
                "replacement_text": "Refined middle paragraph.",
                "selection_anchor": "@main.tex:3:1-3:28"
            }),
            None,
        )
        .await;

        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(result.content["reviewArtifact"], json!(true));
        assert_eq!(
            result.content["reviewArtifactPayload"]["selectionRange"],
            json!("@main.tex:3:1-3:28")
        );

        let updated = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(
            updated,
            [
                "Introduction paragraph.",
                "",
                "Refined middle paragraph.",
                "",
                "TARGET paragraph to refine.",
                "",
                "Closing paragraph.",
            ]
            .join("\n")
        );
    }

    #[tokio::test]
    async fn replace_selected_text_returns_review_artifact_when_approval_is_required() {
        let temp = tempdir().unwrap();
        let project_root = temp.path().to_string_lossy().to_string();
        let file_path = temp.path().join("main.tex");
        let original = "alpha\nbeta\nomega\n";
        tokio::fs::write(&file_path, original.as_bytes())
            .await
            .unwrap();

        let runtime_state = AgentRuntimeState::default();

        let result = execute_replace_selected_text(
            &runtime_state,
            "tab-pending",
            &project_root,
            "call-2",
            json!({
                "path": "main.tex",
                "expected_selected_text": "beta",
                "replacement_text": "BETA",
                "selection_anchor": "@main.tex:2:1-2:5"
            }),
            None,
        )
        .await;

        assert!(result.is_error);
        assert_eq!(result.content["approvalRequired"], json!(true));
        assert_eq!(result.content["reviewArtifact"], json!(true));
        assert_eq!(
            result.content["reviewArtifactPayload"]["oldContent"],
            json!(original)
        );
        assert_eq!(
            result.content["reviewArtifactPayload"]["newContent"],
            json!("alpha\nBETA\nomega\n")
        );

        let current = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(current, original);
    }

    #[tokio::test]
    async fn apply_text_patch_requires_unique_match_and_preserves_rest_of_file() {
        let temp = tempdir().unwrap();
        let project_root = temp.path().to_string_lossy().to_string();
        let file_path = temp.path().join("main.tex");
        let original = "before\nline to patch\nafter\n";
        tokio::fs::write(&file_path, original.as_bytes())
            .await
            .unwrap();

        let runtime_state = AgentRuntimeState::default();
        runtime_state
            .set_tool_approval("tab-patch", "patch_file", "allow_session")
            .await
            .unwrap();

        let result = execute_apply_text_patch(
            &runtime_state,
            "tab-patch",
            &project_root,
            "call-3",
            json!({
                "path": "main.tex",
                "expected_old_text": "line to patch",
                "new_text": "patched line"
            }),
            None,
        )
        .await;

        assert!(!result.is_error, "{:?}", result.content);
        let updated = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(updated, "before\npatched line\nafter\n");
    }
}
