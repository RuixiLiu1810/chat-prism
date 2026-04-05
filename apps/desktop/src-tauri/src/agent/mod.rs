mod chat_completions;
mod document_artifacts;
mod events;
mod openai;
mod provider;
mod review_runtime;
mod session;
mod telemetry;
mod tools;
mod turn_engine;

use tauri::{Emitter, Manager, State, WebviewWindow};
use tokio::sync::watch;

pub use events::{
    AgentCompletePayload, AgentErrorEvent, AgentEventEnvelope, AgentEventPayload, AgentStatusEvent,
    AGENT_COMPLETE_EVENT_NAME, AGENT_EVENT_NAME,
};
pub use provider::{
    AgentResponseMode, AgentSamplingProfile, AgentSelectionScope, AgentStatus, AgentTaskKind,
    AgentTurnDescriptor, AgentTurnProfile,
};
pub use session::{
    AgentRuntimeState, AgentSessionRecord, AgentSessionSummary, AgentSessionWorkState,
};
use turn_engine::{emit_tool_resumed, emit_turn_resumed};

use crate::settings;

const AGENT_CANCELLED_MESSAGE: &str = "Agent run cancelled by user.";

const AGENT_BASE_INSTRUCTIONS: &str = concat!(
    "You are an AI assistant integrated into ChatPrism, a project-aware academic writing and coding workspace. ",
    "Behave like an execution-oriented agent, not a general chat assistant.\n",
    "\n",
    "[Context marker meaning]\n",
    "- [Currently open file: X] means the user is actively working in file X.\n",
    "- [Selection: @X:startLine:startCol-endLine:endCol] means the user selected a span inside file X. Treat the full @... string as the exact selection anchor.\n",
    "- [Selected text: ...] means the user selected that exact text. When using selection-edit tools, preserve it verbatim unless you have re-read the file and verified a different exact match.\n",
    "- [Attached resource: X] means the user attached or pinned a reference resource. It is supporting context, not an active editor selection.\n",
    "- [Resource path: X] gives the project-relative path for an attached resource.\n",
    "- [Attached excerpt: ...] contains a quoted excerpt or extracted text from an attached resource. Treat it as reference material for analysis, extraction, or comparison tasks.\n",
    "- [Relevant resource matches: ...] or [Relevant resource evidence: ...] contains local search hits extracted from attached resources. Treat these hits as high-signal evidence candidates and use them before resorting to shell probing or repeated exploration.\n",
    "\n",
    "[Hard execution rules]\n",
    "1. When the request is an edit or file-work request, take concrete project actions instead of only replying with prose.\n",
    "2. Keep edits small and targeted. Preserve surrounding structure, formatting, and unrelated content.\n",
    "3. If the request targets selected text, you MUST treat it as a selection-scoped edit task unless the user explicitly asks for suggestions only.\n",
    "4. For selection-scoped edits, use precise edit tools such as replace_selected_text or apply_text_patch. Using write_file for a selection-scoped edit is forbidden unless the user explicitly asks for a whole-file rewrite.\n",
    "5. For file-scoped edits without a trusted exact target, read the text file first, then patch it with exact existing text. Do not guess exact text spans.\n",
    "6. Use write_file only for whole-file rewrites, creating a new file, or final apply steps after review.\n",
    "7. If the user explicitly asks for suggestions, analysis, explanation, review-only output, or says not to modify files, stay in suggestion mode and do not call edit tools.\n",
    "8. If a write or shell action is blocked by approval, do not pretend the change was applied. Treat it as staged/pending and wait.\n",
    "9. Do not treat attached resources as active-file selections unless the prompt also contains an explicit [Selection: ...] marker.\n",
    "10. PDFs and DOCX resources are documents, not plain text files. Use read_document instead of read_file when you need evidence from an attached document.\n",
    "\n",
    "[Internal tool-use checklist]\n",
    "Before every tool call, verify internally:\n",
    "- which file you are acting on\n",
    "- whether you already read the current file content when exact matching is required\n",
    "- whether expected_old_text or expected_selected_text matches the current file verbatim, including whitespace and line breaks\n",
    "Do not reveal this checklist unless the user asks for your reasoning.\n",
    "\n",
    "[Response style]\n",
    "Keep responses concise when the real value is in the tool action and resulting reviewable change.\n"
);

fn prompt_lower(prompt: &str) -> String {
    prompt.to_lowercase()
}

fn has_selection_context(prompt: &str) -> bool {
    prompt.contains("[Selection:")
}

fn has_attachment_context(prompt: &str) -> bool {
    prompt.contains("[Attached resource:")
}

fn has_binary_attachment_context(prompt: &str) -> bool {
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

fn prompt_explicitly_requests_suggestions(prompt: &str) -> bool {
    let lower = prompt_lower(prompt);
    let en = [
        "suggest",
        "give me a version",
        "show me a version",
        "propose",
        "brainstorm",
        "review only",
        "do not modify",
        "don't modify",
        "without editing",
        "without changing the file",
    ];
    let zh = [
        "建议",
        "给我几个",
        "有没有更好",
        "可以怎么",
        "怎么改比较好",
        "不要改文件",
        "只是看看",
        "解释一下",
        "分析一下",
        "仅建议",
        "只做建议",
        "只看一下",
    ];
    en.iter().any(|needle| lower.contains(needle))
        || zh.iter().any(|needle| prompt.contains(needle))
}

fn prompt_explicitly_requests_edit(prompt: &str) -> bool {
    let lower = prompt_lower(prompt);
    let en = [
        "refine",
        "rewrite",
        "edit",
        "polish",
        "improve",
        "fix",
        "revise",
        "shorten",
        "tighten",
        "rephrase",
        "proofread",
        "clean up",
    ];
    let zh = [
        "修改",
        "改成",
        "改为",
        "润色",
        "优化",
        "精简",
        "修正",
        "完善",
        "重写",
        "调整",
        "修一下",
        "帮我改",
        "改一下",
        "润一下",
    ];
    en.iter().any(|needle| lower.contains(needle))
        || zh.iter().any(|needle| prompt.contains(needle))
}

fn prompt_explicitly_requests_deep_analysis(prompt: &str) -> bool {
    let lower = prompt_lower(prompt);
    let en = [
        "detailed",
        "deep",
        "deeper",
        "compare",
        "comparison",
        "synthesize",
        "summary",
        "summarize",
        "which paper",
        "which article",
        "evidence",
        "walk me through",
    ];
    let zh = [
        "详细",
        "深入",
        "展开",
        "对比",
        "比较",
        "总结",
        "归纳",
        "综述",
        "哪篇",
        "哪一篇",
        "列出",
        "依据",
    ];
    en.iter().any(|needle| lower.contains(needle))
        || zh.iter().any(|needle| prompt.contains(needle))
}

fn has_relevant_resource_evidence(prompt: &str) -> bool {
    prompt.contains("[Relevant resource evidence:")
        || prompt.contains("[Relevant resource matches:")
}

pub fn tool_choice_for_task(
    request: &AgentTurnDescriptor,
    profile: &AgentTurnProfile,
) -> &'static str {
    match profile.task_kind {
        AgentTaskKind::SelectionEdit | AgentTaskKind::FileEdit => "required",
        AgentTaskKind::SuggestionOnly => "none",
        AgentTaskKind::Analysis
            if has_attachment_context(&request.prompt)
                && has_relevant_resource_evidence(&request.prompt) =>
        {
            "none"
        }
        AgentTaskKind::Analysis if has_binary_attachment_context(&request.prompt) => "required",
        AgentTaskKind::General | AgentTaskKind::Analysis => "auto",
    }
}

pub fn max_rounds_for_task(profile: &AgentTurnProfile) -> u32 {
    match profile.task_kind {
        AgentTaskKind::SuggestionOnly => 2,
        AgentTaskKind::SelectionEdit => 8,
        AgentTaskKind::FileEdit => 10,
        AgentTaskKind::Analysis | AgentTaskKind::General => 10,
    }
}

pub fn resolve_turn_profile(request: &AgentTurnDescriptor) -> AgentTurnProfile {
    let mut profile = request.turn_profile.clone().unwrap_or_default();

    if profile.selection_scope == AgentSelectionScope::None
        && has_selection_context(&request.prompt)
    {
        profile.selection_scope = AgentSelectionScope::SelectedSpan;
    }

    if profile.task_kind == AgentTaskKind::General {
        if profile.selection_scope == AgentSelectionScope::SelectedSpan {
            if prompt_explicitly_requests_suggestions(&request.prompt) {
                profile.task_kind = AgentTaskKind::SuggestionOnly;
            } else if prompt_explicitly_requests_edit(&request.prompt) {
                profile.task_kind = AgentTaskKind::SelectionEdit;
            }
        } else if has_attachment_context(&request.prompt) {
            if prompt_explicitly_requests_suggestions(&request.prompt) {
                profile.task_kind = AgentTaskKind::SuggestionOnly;
            } else {
                profile.task_kind = AgentTaskKind::Analysis;
            }
        } else if prompt_explicitly_requests_suggestions(&request.prompt) {
            profile.task_kind = AgentTaskKind::SuggestionOnly;
        }
    }

    if profile.response_mode == AgentResponseMode::Default {
        profile.response_mode = match profile.task_kind {
            AgentTaskKind::SelectionEdit | AgentTaskKind::FileEdit => {
                AgentResponseMode::ReviewableChange
            }
            AgentTaskKind::SuggestionOnly => AgentResponseMode::SuggestionOnly,
            AgentTaskKind::General | AgentTaskKind::Analysis => AgentResponseMode::Default,
        };
    }

    if profile.sampling_profile == AgentSamplingProfile::Default {
        profile.sampling_profile = match profile.task_kind {
            AgentTaskKind::SelectionEdit | AgentTaskKind::FileEdit => {
                AgentSamplingProfile::EditStable
            }
            AgentTaskKind::SuggestionOnly | AgentTaskKind::Analysis => {
                if has_attachment_context(&request.prompt)
                    || prompt_explicitly_requests_deep_analysis(&request.prompt)
                {
                    AgentSamplingProfile::AnalysisDeep
                } else {
                    AgentSamplingProfile::AnalysisBalanced
                }
            }
            AgentTaskKind::General => AgentSamplingProfile::Default,
        };
    }

    profile
}

pub fn build_agent_instructions_with_work_state(
    request: &AgentTurnDescriptor,
    work_state: Option<&AgentSessionWorkState>,
) -> String {
    let mut instructions = AGENT_BASE_INSTRUCTIONS.to_string();
    let profile = resolve_turn_profile(request);

    match profile.task_kind {
        AgentTaskKind::SelectionEdit => {
            instructions.push_str(
                "This turn is classified as a selection-scoped edit task. \
You must aim for a reviewable file change, not a prose-only rewritten paragraph. \
Use replace_selected_text when the selected text and selection anchor are trustworthy. \
If you need to verify the exact file content first, call read_file, then use apply_text_patch with an exact verbatim match. \
Do not use write_file for this selection-scoped edit unless the user explicitly asks for a whole-file rewrite.\n",
            );
        }
        AgentTaskKind::FileEdit => {
            instructions.push_str(
                "This turn is classified as a file-edit request. \
Produce a reviewable file change instead of a prose-only explanation whenever possible. \
Read the file before exact-match patching, and reserve write_file for whole-file rewrites or final apply steps.\n",
            );
        }
        AgentTaskKind::SuggestionOnly => {
            instructions.push_str(
                "This turn is classified as suggestion-only. \
Stay in suggestion mode and avoid edit tools unless the user explicitly asks to apply the change.\n",
            );
        }
        AgentTaskKind::Analysis => {
            instructions.push_str(
                "This turn is classified as analysis. Prefer clear reasoning and targeted file reads over file edits unless the user explicitly asks for changes.\n",
            );
        }
        AgentTaskKind::General => {
            if profile.selection_scope == AgentSelectionScope::SelectedSpan {
                instructions.push_str(
                    "This turn includes selected text context. Treat the selection as high-signal context, but only perform file edits when the request clearly calls for modification.\n",
                );
            }
        }
    }

    if has_attachment_context(&request.prompt) {
        instructions.push_str(
            "This turn includes attached resources. Ground your answer in those resources, cite which attached file supports each key conclusion, and synthesize evidence before concluding.\n",
        );
        instructions.push_str(
            "When answering a document/resource question, prefer this structure when it fits: Matching documents, Supporting evidence (cite the attached file plus the page or paragraph label from the evidence block), then Conclusion. If the ingested evidence is insufficient, say that clearly instead of inventing tool calls or shell steps.\n",
        );
        if has_binary_attachment_context(&request.prompt) {
            instructions.push_str(
                "For attached PDFs or DOCX resources, use read_document when additional evidence is needed. Do not call read_file on binary files. Do not use shell commands such as pdftotext for exploratory extraction unless the user explicitly asks for command-line inspection. If read_document reports fallback extraction, treat it as runtime-managed evidence gathering.\n",
            );
            instructions.push_str(
                "[Document analysis strategy]\n\
                When the user asks you to analyze, summarize, extract information from, or answer questions about attached PDF or DOCX documents:\n\
                1. First call inspect_resource to check document metadata and extraction status.\n\
                2. For summary/overview tasks, start with read_document (with or without a query) to get the excerpt.\n\
                3. For specific information queries, call search_document_text with relevant keywords. Search multiple times with different keywords if needed to build a complete picture.\n\
                4. Do NOT rely solely on pre-extracted excerpts. Proactively search for key topics, main points, methods, results, conclusions, recommendations based on what the user is asking.\n\
                5. If search returns no results for a term, try alternative keywords or broader searches.\n\
                6. Synthesize all found evidence into a comprehensive answer that directly addresses the user's question.\n"
            );
        }
    }

    if profile.sampling_profile == AgentSamplingProfile::AnalysisDeep {
        instructions.push_str(
            "Use a deeper analysis style for this turn: inspect evidence carefully, compare relevant sources when useful, and avoid stopping at a one-line conclusion when the user is asking a research or document question.\n",
        );
    }

    if let Some(work_state) = work_state {
        let recall_lines = selective_session_recall(request, work_state);
        if !recall_lines.is_empty() {
            if !instructions.ends_with('\n') {
                instructions.push('\n');
            }
            instructions.push_str("[Selective session recall]\n");
            instructions.push_str(
                "Use these continuity hints to stay aligned with the active task and avoid repeating already completed exploration unless the current request truly requires it.\n",
            );
            for line in recall_lines {
                instructions.push_str("- ");
                instructions.push_str(&line);
                instructions.push('\n');
            }
            instructions.push('\n');
        }
    }

    instructions
}

fn push_unique_recall_line(lines: &mut Vec<String>, line: Option<String>) {
    let Some(line) = line.map(|value| value.trim().to_string()) else {
        return;
    };
    if line.is_empty() || lines.iter().any(|existing| existing == &line) {
        return;
    }
    lines.push(line);
}

fn selective_session_recall(
    request: &AgentTurnDescriptor,
    work_state: &AgentSessionWorkState,
) -> Vec<String> {
    let profile = resolve_turn_profile(request);
    let current_request_objective = summarize_objective(&request.prompt);
    let mut lines = Vec::new();

    if let Some(pending_state) = work_state.pending_state.as_deref() {
        let pending_tool = work_state.pending_tool_name.as_deref().unwrap_or("tool");
        let pending_target = work_state
            .pending_target
            .as_deref()
            .map(|value| format!(" on {}", value))
            .unwrap_or_default();
        push_unique_recall_line(
            &mut lines,
            Some(format!(
                "Pending state: {} via {}{}",
                pending_state, pending_tool, pending_target
            )),
        );
    }

    if let Some(recent_objective) = work_state.recent_objective.as_deref() {
        push_unique_recall_line(
            &mut lines,
            Some(format!("Recent objective: {}", recent_objective)),
        );
    }

    let should_recall_target = matches!(
        profile.task_kind,
        AgentTaskKind::SelectionEdit | AgentTaskKind::FileEdit | AgentTaskKind::Analysis
    ) || work_state.pending_state.is_some();
    if should_recall_target {
        if let Some(target) = work_state.current_target.as_deref() {
            push_unique_recall_line(&mut lines, Some(format!("Working target: {}", target)));
        }
    }

    if let Some(activity) = work_state.last_tool_activity.as_deref() {
        let should_include_activity = work_state.pending_state.is_some()
            || matches!(
                profile.task_kind,
                AgentTaskKind::Analysis | AgentTaskKind::General
            )
            || lines.len() < 2;
        if should_include_activity {
            push_unique_recall_line(
                &mut lines,
                Some(format!("Recent tool activity: {}", activity)),
            );
        }
    }

    if let Some(objective) = work_state.current_objective.as_deref() {
        if current_request_objective.as_deref() != Some(objective) {
            push_unique_recall_line(&mut lines, Some(format!("Active objective: {}", objective)));
        }
    }

    if lines.len() > 4 {
        lines.truncate(4);
    }

    lines
}

#[allow(dead_code)]
pub fn build_agent_instructions(request: &AgentTurnDescriptor) -> String {
    build_agent_instructions_with_work_state(request, None)
}

pub async fn agent_instructions_for_request(
    state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
) -> String {
    let work_state = state
        .work_state_for_prompt(&request.tab_id, request.local_session_id.as_deref())
        .await;
    build_agent_instructions_with_work_state(request, Some(&work_state))
}

fn emit_agent_event(window: &WebviewWindow, tab_id: &str, payload: AgentEventPayload) {
    let _ = window.emit(
        AGENT_EVENT_NAME,
        AgentEventEnvelope {
            tab_id: tab_id.to_string(),
            payload,
        },
    );
}

fn emit_agent_complete(window: &WebviewWindow, tab_id: &str, outcome: &str) {
    let _ = window.emit(
        AGENT_COMPLETE_EVENT_NAME,
        AgentCompletePayload {
            tab_id: tab_id.to_string(),
            outcome: outcome.to_string(),
        },
    );
}

#[cfg(test)]
use self::openai::OpenAiProvider;

#[cfg(test)]
fn openai_provider() -> OpenAiProvider {
    OpenAiProvider
}

fn selected_provider(
    app: &tauri::AppHandle,
    project_path: Option<&str>,
) -> Result<settings::AgentRuntimeConfig, String> {
    settings::load_agent_runtime(app, project_path)
}

fn selected_status(app: &tauri::AppHandle) -> Result<AgentStatus, String> {
    let runtime = selected_provider(app, None)?;
    Ok(match runtime.provider.as_str() {
        "openai" => openai::runtime_status(app),
        "minimax" | "deepseek" => chat_completions::runtime_status(app, &runtime.provider),
        other => AgentStatus {
            provider: other.to_string(),
            display_name: "Unsupported Provider".to_string(),
            ready: false,
            mode: "unsupported_provider".to_string(),
            message: format!("Unsupported agent provider in settings: {}.", other),
            default_model: Some(runtime.model),
        },
    })
}

async fn dispatch_run_turn_loop(
    window: &WebviewWindow,
    state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    history: &[serde_json::Value],
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<openai::AgentTurnOutcome, String> {
    let runtime = selected_provider(&window.app_handle(), Some(&request.project_path))?;
    match runtime.provider.as_str() {
        "openai" => openai::run_turn_loop(window, state, request, cancel_rx).await,
        "minimax" | "deepseek" => {
            chat_completions::run_turn_loop(window, state, request, history, cancel_rx).await
        }
        other => Err(format!(
            "Unsupported agent provider in settings: {}.",
            other
        )),
    }
}

async fn dispatch_cancel_turn(
    app: &tauri::AppHandle,
    state: &AgentRuntimeState,
    tab_id: &str,
    response_id: Option<&str>,
) -> Result<(), String> {
    let runtime = selected_provider(app, None)?;
    match runtime.provider.as_str() {
        "openai" => {
            if let Some(response_id) = response_id {
                openai::cancel_response(app, response_id).await
            } else {
                Ok(())
            }
        }
        "minimax" | "deepseek" => {
            if state.cancel_tab(tab_id).await {
                Ok(())
            } else {
                chat_completions::cancel_response(app, &runtime.provider).await
            }
        }
        other => Err(format!(
            "Unsupported agent provider in settings: {}.",
            other
        )),
    }
}

fn summarize_session_title(prompt: &str) -> String {
    let first_line = prompt.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        return "New Chat".to_string();
    }
    let title = if first_line.chars().count() > 48 {
        format!("{}...", first_line.chars().take(48).collect::<String>())
    } else {
        first_line.to_string()
    };
    if title.is_empty() {
        "New Chat".to_string()
    } else {
        title
    }
}

fn summarize_objective(prompt: &str) -> Option<String> {
    prompt
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('['))
        .map(|line| {
            if line.chars().count() > 120 {
                format!("{}...", line.chars().take(120).collect::<String>())
            } else {
                line.to_string()
            }
        })
}

async fn record_request_objective(
    state: &AgentRuntimeState,
    tab_id: &str,
    local_session_id: Option<&str>,
    prompt: &str,
) {
    state
        .set_current_objective(tab_id, local_session_id, summarize_objective(prompt))
        .await;
}

#[cfg(test)]
mod prompt_tests {
    use super::{
        build_agent_instructions, build_agent_instructions_with_work_state, resolve_turn_profile,
        tool_choice_for_task,
    };
    use crate::agent::provider::{
        AgentResponseMode, AgentSelectionScope, AgentTaskKind, AgentTurnDescriptor,
        AgentTurnProfile,
    };
    use crate::agent::session::AgentSessionWorkState;

    fn make_request(prompt: &str, turn_profile: Option<AgentTurnProfile>) -> AgentTurnDescriptor {
        AgentTurnDescriptor {
            project_path: "/tmp/project".to_string(),
            prompt: prompt.to_string(),
            tab_id: "tab-1".to_string(),
            model: None,
            local_session_id: None,
            previous_response_id: None,
            turn_profile,
        }
    }

    #[test]
    fn build_agent_instructions_biases_selection_edit_requests_toward_precise_edit_tools() {
        let prompt = "[Currently open file: main.tex]\n[Selection: @main.tex:14:1-14:20]\n[Selected text:\nfoo\n]\n\nrefine this paragraph";
        let instructions = build_agent_instructions(&make_request(prompt, None));
        assert!(instructions.contains("replace_selected_text"));
        assert!(instructions.contains("prose-only rewritten paragraph"));
    }

    #[test]
    fn build_agent_instructions_does_not_force_edit_tools_for_suggestion_only_requests() {
        let prompt = "[Currently open file: main.tex]\n[Selection: @main.tex:14:1-14:20]\n[Selected text:\nfoo\n]\n\nsuggest a better version of this paragraph";
        let instructions = build_agent_instructions(&make_request(prompt, None));
        assert!(instructions.contains("[Hard execution rules]"));
        assert!(instructions.contains("suggestion-only"));
    }

    #[test]
    fn build_agent_instructions_honors_explicit_selection_edit_profile() {
        let prompt = "[Currently open file: main.tex]\n[Selection: @main.tex:14:1-14:20]\n[Selected text:\nfoo\n]\n\nrefine this paragraph";
        let instructions = build_agent_instructions(&make_request(
            prompt,
            Some(AgentTurnProfile {
                task_kind: AgentTaskKind::SelectionEdit,
                selection_scope: AgentSelectionScope::SelectedSpan,
                response_mode: AgentResponseMode::ReviewableChange,
                ..AgentTurnProfile::default()
            }),
        ));
        assert!(instructions.contains("classified as a selection-scoped edit task"));
        assert!(instructions.contains("replace_selected_text"));
    }

    #[test]
    fn build_agent_instructions_honors_explicit_suggestion_profile() {
        let prompt = "[Currently open file: main.tex]\n[Selection: @main.tex:14:1-14:20]\n[Selected text:\nfoo\n]\n\nrefine this paragraph";
        let instructions = build_agent_instructions(&make_request(
            prompt,
            Some(AgentTurnProfile {
                task_kind: AgentTaskKind::SuggestionOnly,
                selection_scope: AgentSelectionScope::SelectedSpan,
                response_mode: AgentResponseMode::SuggestionOnly,
                ..AgentTurnProfile::default()
            }),
        ));
        assert!(instructions.contains("classified as suggestion-only"));
        assert!(instructions.contains("Stay in suggestion mode and avoid edit tools"));
    }

    #[test]
    fn build_agent_instructions_understands_chinese_edit_fallback() {
        let prompt = "[Currently open file: main.tex]\n[Selection: @main.tex:14:1-14:20]\n[Selected text:\nfoo\n]\n\n帮我润色这段话";
        let instructions = build_agent_instructions(&make_request(prompt, None));
        assert!(instructions.contains("selection-scoped edit task"));
        assert!(instructions.contains("replace_selected_text"));
    }

    #[test]
    fn tool_choice_matches_task_kind() {
        use super::tool_choice_for_task;
        use crate::agent::provider::AgentSamplingProfile;

        let selection_request = make_request(
            "[Currently open file: main.tex]\n[Selection: @main.tex:1:1-1:4]\n[Selected text:\ntext\n]\n\nrefine this",
            None,
        );
        assert_eq!(
            tool_choice_for_task(
                &selection_request,
                &AgentTurnProfile {
                    task_kind: AgentTaskKind::SelectionEdit,
                    selection_scope: AgentSelectionScope::SelectedSpan,
                    response_mode: AgentResponseMode::ReviewableChange,
                    sampling_profile: AgentSamplingProfile::EditStable,
                    source_hint: None,
                }
            ),
            "required"
        );
        let suggestion_request = make_request("suggest alternatives", None);
        assert_eq!(
            tool_choice_for_task(
                &suggestion_request,
                &AgentTurnProfile {
                    task_kind: AgentTaskKind::SuggestionOnly,
                    selection_scope: AgentSelectionScope::None,
                    response_mode: AgentResponseMode::SuggestionOnly,
                    sampling_profile: AgentSamplingProfile::AnalysisBalanced,
                    source_hint: None,
                }
            ),
            "none"
        );
    }

    #[test]
    fn binary_attachment_analysis_prefers_prompt_evidence_over_extra_tool_turns() {
        let request = make_request(
            "[Attached resource: @paper.pdf (pdf)]\n[Resource path: attachments/paper.pdf]\n[Attached excerpt:\nhydrophobic surface treatment\n]\n[Relevant resource evidence:\n- Document: attachments/paper.pdf (pdf)\n  - Page 4: hydrophobic surface treatment was evaluated by contact angle measurements.\n]\n\n哪篇文章提到疏水性相关实验",
            Some(AgentTurnProfile {
                task_kind: AgentTaskKind::Analysis,
                selection_scope: AgentSelectionScope::None,
                response_mode: AgentResponseMode::Default,
                ..AgentTurnProfile::default()
            }),
        );

        assert_eq!(
            tool_choice_for_task(&request, &resolve_turn_profile(&request)),
            "none"
        );
        let instructions = build_agent_instructions(&request);
        assert!(instructions.contains("read_document"));
        assert!(instructions.contains("Do not call read_file on binary files."));
        assert!(instructions.contains("shell commands such as pdftotext"));
        assert!(instructions.contains("Matching documents, Supporting evidence"));
    }

    #[test]
    fn binary_attachment_analysis_without_evidence_requires_document_tool_use() {
        let request = make_request(
            "[Attached resource: @paper.pdf (pdf)]\n[Resource path: attachments/paper.pdf]\n[Attached excerpt:\nhydrophobic surface treatment\n]\n\n哪篇文章提到疏水性相关实验",
            Some(AgentTurnProfile {
                task_kind: AgentTaskKind::Analysis,
                selection_scope: AgentSelectionScope::None,
                response_mode: AgentResponseMode::Default,
                ..AgentTurnProfile::default()
            }),
        );

        assert_eq!(
            tool_choice_for_task(&request, &resolve_turn_profile(&request)),
            "required"
        );
    }

    #[test]
    fn selective_session_recall_prefers_recent_objective_over_echoing_current_prompt() {
        let request = make_request(
            "[Attached resource: @paper.pdf]\n[Resource path: attachments/paper.pdf]\n[Attached excerpt:\nhydrophobic surface treatment\n]\n\nWhich paper mentions hydrophobic experiments?",
            Some(AgentTurnProfile {
                task_kind: AgentTaskKind::Analysis,
                sampling_profile: crate::agent::provider::AgentSamplingProfile::AnalysisDeep,
                ..AgentTurnProfile::default()
            }),
        );
        let work_state = AgentSessionWorkState {
            current_objective: Some("Which paper mentions hydrophobic experiments?".to_string()),
            recent_objective: Some(
                "Compare the attached papers for hydrophobic experiments.".to_string(),
            ),
            current_target: Some("attachments/TiO2 CuS PDA.pdf".to_string()),
            last_tool_activity: Some(
                "Completed run_shell_command on attachments/TiO2 CuS PDA.pdf".to_string(),
            ),
            pending_state: None,
            pending_tool_name: None,
            pending_target: None,
        };

        let instructions = build_agent_instructions_with_work_state(&request, Some(&work_state));
        assert!(instructions.contains("[Selective session recall]"));
        assert!(instructions.contains(
            "Recent objective: Compare the attached papers for hydrophobic experiments."
        ));
        assert!(!instructions
            .contains("Active objective: Which paper mentions hydrophobic experiments?"));
    }

    #[test]
    fn selective_session_recall_includes_pending_state_and_target_for_edit_turns() {
        let request = make_request(
            "[Currently open file: main.tex]\n[Selection: @main.tex:10:1-10:20]\n[Selected text:\nfoo\n]\n\nrefine this paragraph",
            Some(AgentTurnProfile {
                task_kind: AgentTaskKind::SelectionEdit,
                selection_scope: AgentSelectionScope::SelectedSpan,
                response_mode: AgentResponseMode::ReviewableChange,
                ..AgentTurnProfile::default()
            }),
        );
        let work_state = AgentSessionWorkState {
            current_objective: Some("refine this paragraph".to_string()),
            recent_objective: Some("tighten the related work section".to_string()),
            current_target: Some("main.tex".to_string()),
            last_tool_activity: Some("Completed apply_text_patch on main.tex".to_string()),
            pending_state: Some("review_ready".to_string()),
            pending_tool_name: Some("patch_file".to_string()),
            pending_target: Some("main.tex".to_string()),
        };

        let instructions = build_agent_instructions_with_work_state(&request, Some(&work_state));
        assert!(instructions.contains("Pending state: review_ready via patch_file on main.tex"));
        assert!(instructions.contains("Working target: main.tex"));
        assert!(instructions.contains("Recent objective: tighten the related work section"));
    }

    #[test]
    fn document_analysis_includes_proactive_search_strategy() {
        let request = make_request(
            "[Attached resource: @research.pdf (pdf)]\n[Resource path: attachments/research.pdf]\n[Attached excerpt:\nIntroduction and methods\n]\n\nSummarize the key findings and recommendations from this paper.",
            Some(AgentTurnProfile {
                task_kind: AgentTaskKind::Analysis,
                selection_scope: AgentSelectionScope::None,
                response_mode: AgentResponseMode::Default,
                ..AgentTurnProfile::default()
            }),
        );

        let instructions = build_agent_instructions(&request);
        // Verify document analysis strategy is present
        assert!(instructions.contains("[Document analysis strategy]"));
        assert!(instructions.contains("inspect_resource"));
        assert!(instructions.contains("read_document"));
        assert!(instructions.contains("search_document_text"));
        assert!(instructions.contains("Search multiple times with different keywords"));
        assert!(instructions.contains("Synthesize all found evidence into a comprehensive answer"));
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSmokeStep {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSmokeResult {
    pub provider: String,
    pub runtime_mode: String,
    pub ok: bool,
    pub steps: Vec<AgentSmokeStep>,
}

#[tauri::command]
pub async fn agent_check_status(app: tauri::AppHandle) -> Result<AgentStatus, String> {
    selected_status(&app)
}

#[tauri::command]
pub async fn agent_start_turn(
    window: WebviewWindow,
    state: State<'_, AgentRuntimeState>,
    project_path: String,
    prompt: String,
    tab_id: String,
    model: Option<String>,
    turn_profile: Option<AgentTurnProfile>,
) -> Result<String, String> {
    state.ensure_storage(&window.app_handle()).await?;
    let runtime = selected_provider(&window.app_handle(), Some(&project_path))?;
    let request = AgentTurnDescriptor {
        project_path,
        prompt,
        tab_id: tab_id.clone(),
        model,
        local_session_id: None,
        previous_response_id: None,
        turn_profile,
    };

    emit_agent_event(
        &window,
        &tab_id,
        AgentEventPayload::Status(AgentStatusEvent {
            stage: "queued".to_string(),
            message: "Agent runtime received the request.".to_string(),
        }),
    );

    record_request_objective(&state, &tab_id, None, &request.prompt).await;

    let cancel_rx = state.register_cancellation(&tab_id).await;
    let outcome = dispatch_run_turn_loop(&window, &state, &request, &[], Some(cancel_rx)).await;
    state.clear_cancellation(&tab_id).await;

    match outcome {
        Ok(outcome) => {
            let selected_model = request
                .model
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| runtime.model.clone());

            let session_title = summarize_session_title(&request.prompt);
            let mut session = AgentSessionRecord::new(
                &runtime.provider,
                request.project_path.clone(),
                request.tab_id.clone(),
                session_title,
                selected_model,
            );
            session.touch_response(outcome.response_id.clone());
            let local_session_id = session.local_session_id.clone();
            let mut sessions = state.sessions.lock().await;
            sessions.insert(session.local_session_id.clone(), session);
            drop(sessions);
            state
                .bind_tab_state_to_session(&tab_id, &local_session_id)
                .await;
            state
                .append_history(&local_session_id, outcome.messages)
                .await;

            emit_agent_complete(
                &window,
                &tab_id,
                if outcome.suspended {
                    "suspended"
                } else {
                    "completed"
                },
            );
            Ok(local_session_id)
        }
        Err(message) => {
            if message == AGENT_CANCELLED_MESSAGE {
                emit_agent_complete(&window, &tab_id, "cancelled");
                return Err(message);
            }
            let error_message = message.clone();
            emit_agent_event(
                &window,
                &tab_id,
                AgentEventPayload::Error(AgentErrorEvent {
                    code: "agent_turn_failed".to_string(),
                    message: error_message,
                }),
            );
            emit_agent_complete(&window, &tab_id, "error");
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn agent_continue_turn(
    window: WebviewWindow,
    state: State<'_, AgentRuntimeState>,
    project_path: String,
    prompt: String,
    tab_id: String,
    model: Option<String>,
    local_session_id: Option<String>,
    previous_response_id: Option<String>,
    turn_profile: Option<AgentTurnProfile>,
) -> Result<String, String> {
    state.ensure_storage(&window.app_handle()).await?;
    let runtime = selected_provider(&window.app_handle(), Some(&project_path))?;
    let request = AgentTurnDescriptor {
        project_path,
        prompt,
        tab_id: tab_id.clone(),
        model,
        local_session_id,
        previous_response_id,
        turn_profile,
    };

    emit_agent_event(
        &window,
        &tab_id,
        AgentEventPayload::Status(AgentStatusEvent {
            stage: "queued".to_string(),
            message: "Agent continuation request received.".to_string(),
        }),
    );
    record_request_objective(
        &state,
        &tab_id,
        request.local_session_id.as_deref(),
        &request.prompt,
    )
    .await;

    let current_previous_response_id = if request.previous_response_id.is_some() {
        request.previous_response_id.clone()
    } else if let Some(local_session_id) = request.local_session_id.as_ref() {
        let sessions = state.sessions.lock().await;
        sessions
            .get(local_session_id)
            .and_then(|session| session.last_response_id.clone())
    } else {
        None
    };

    let request = AgentTurnDescriptor {
        previous_response_id: current_previous_response_id,
        ..request
    };

    let prior_history = if let Some(local_session_id) = request.local_session_id.as_ref() {
        state
            .history_for_session(local_session_id)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let cancel_rx = state.register_cancellation(&tab_id).await;
    let outcome =
        dispatch_run_turn_loop(&window, &state, &request, &prior_history, Some(cancel_rx)).await;
    state.clear_cancellation(&tab_id).await;

    match outcome {
        Ok(outcome) => {
            let selected_model = request
                .model
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| runtime.model.clone());

            let mut sessions = state.sessions.lock().await;
            let local_session_id = if let Some(local_session_id) = request.local_session_id.as_ref()
            {
                if let Some(session) = sessions.get_mut(local_session_id) {
                    session.touch_response(outcome.response_id.clone());
                    local_session_id.clone()
                } else {
                    let mut session = AgentSessionRecord::new(
                        &runtime.provider,
                        request.project_path.clone(),
                        request.tab_id.clone(),
                        summarize_session_title(&request.prompt),
                        selected_model,
                    );
                    session.touch_response(outcome.response_id.clone());
                    let local_session_id = session.local_session_id.clone();
                    sessions.insert(session.local_session_id.clone(), session);
                    local_session_id
                }
            } else {
                let mut session = AgentSessionRecord::new(
                    &runtime.provider,
                    request.project_path.clone(),
                    request.tab_id.clone(),
                    summarize_session_title(&request.prompt),
                    selected_model,
                );
                session.touch_response(outcome.response_id.clone());
                let local_session_id = session.local_session_id.clone();
                sessions.insert(session.local_session_id.clone(), session);
                local_session_id
            };
            drop(sessions);
            state
                .bind_tab_state_to_session(&tab_id, &local_session_id)
                .await;
            state
                .append_history(&local_session_id, outcome.messages)
                .await;

            emit_agent_complete(
                &window,
                &tab_id,
                if outcome.suspended {
                    "suspended"
                } else {
                    "completed"
                },
            );
            Ok(local_session_id)
        }
        Err(message) => {
            if message == AGENT_CANCELLED_MESSAGE {
                emit_agent_complete(&window, &tab_id, "cancelled");
                return Err(message);
            }
            let error_message = message.clone();
            emit_agent_event(
                &window,
                &tab_id,
                AgentEventPayload::Error(AgentErrorEvent {
                    code: "agent_turn_failed".to_string(),
                    message: error_message,
                }),
            );
            emit_agent_complete(&window, &tab_id, "error");
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn agent_cancel_turn(
    window: WebviewWindow,
    state: State<'_, AgentRuntimeState>,
    tab_id: String,
    response_id: Option<String>,
) -> Result<(), String> {
    if let Err(message) = dispatch_cancel_turn(
        &window.app_handle(),
        &state,
        &tab_id,
        response_id.as_deref(),
    )
    .await
    {
        emit_agent_event(
            &window,
            &tab_id,
            AgentEventPayload::Error(AgentErrorEvent {
                code: "agent_cancel_failed".to_string(),
                message,
            }),
        );
    }

    emit_agent_complete(&window, &tab_id, "cancelled");
    Ok(())
}

#[tauri::command]
pub async fn agent_set_tool_approval(
    app: tauri::AppHandle,
    state: State<'_, AgentRuntimeState>,
    tab_id: String,
    tool_name: String,
    decision: String,
) -> Result<(), String> {
    state.ensure_storage(&app).await?;
    state
        .set_tool_approval(&tab_id, &tool_name, &decision)
        .await
}

#[tauri::command]
pub async fn agent_resume_pending_turn(
    window: WebviewWindow,
    state: State<'_, AgentRuntimeState>,
    tab_id: String,
) -> Result<String, String> {
    state.ensure_storage(&window.app_handle()).await?;
    let Some(pending) = state.take_pending_turn(&tab_id).await else {
        return Err("No pending approved turn to resume.".to_string());
    };
    let pending_for_retry = pending.clone();

    emit_turn_resumed(
        Some(&window),
        &tab_id,
        pending.local_session_id.as_deref(),
        "Resuming the suspended turn after approval.",
    );
    emit_tool_resumed(
        Some(&window),
        &tab_id,
        &pending.approval_tool_name,
        pending.target_label.as_deref(),
        "Approved tool is resuming in the current turn.",
    );
    emit_agent_event(
        &window,
        &tab_id,
        AgentEventPayload::Status(AgentStatusEvent {
            stage: "resuming_after_approval".to_string(),
            message: "Resuming the suspended turn after approval...".to_string(),
        }),
    );

    let runtime = selected_provider(&window.app_handle(), Some(&pending.project_path))?;
    let request = AgentTurnDescriptor {
        project_path: pending.project_path.clone(),
        prompt: pending.continuation_prompt.clone(),
        tab_id: tab_id.clone(),
        model: pending.model.clone(),
        local_session_id: pending.local_session_id.clone(),
        previous_response_id: None,
        turn_profile: pending.turn_profile.clone(),
    };

    let current_previous_response_id =
        if let Some(local_session_id) = request.local_session_id.as_ref() {
            let sessions = state.sessions.lock().await;
            sessions
                .get(local_session_id)
                .and_then(|session| session.last_response_id.clone())
        } else {
            None
        };
    let request = AgentTurnDescriptor {
        previous_response_id: current_previous_response_id,
        ..request
    };
    let prior_history = if let Some(local_session_id) = request.local_session_id.as_ref() {
        state
            .history_for_session(local_session_id)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let cancel_rx = state.register_cancellation(&tab_id).await;
    let outcome =
        dispatch_run_turn_loop(&window, &state, &request, &prior_history, Some(cancel_rx)).await;
    state.clear_cancellation(&tab_id).await;

    match outcome {
        Ok(outcome) => {
            let selected_model = request
                .model
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| runtime.model.clone());

            let mut sessions = state.sessions.lock().await;
            let local_session_id = if let Some(local_session_id) = request.local_session_id.as_ref()
            {
                if let Some(session) = sessions.get_mut(local_session_id) {
                    session.touch_response(outcome.response_id.clone());
                    local_session_id.clone()
                } else {
                    let mut session = AgentSessionRecord::new(
                        &runtime.provider,
                        request.project_path.clone(),
                        request.tab_id.clone(),
                        summarize_session_title(&request.prompt),
                        selected_model,
                    );
                    session.touch_response(outcome.response_id.clone());
                    let local_session_id = session.local_session_id.clone();
                    sessions.insert(session.local_session_id.clone(), session);
                    local_session_id
                }
            } else {
                let mut session = AgentSessionRecord::new(
                    &runtime.provider,
                    request.project_path.clone(),
                    request.tab_id.clone(),
                    summarize_session_title(&request.prompt),
                    selected_model,
                );
                session.touch_response(outcome.response_id.clone());
                let local_session_id = session.local_session_id.clone();
                sessions.insert(session.local_session_id.clone(), session);
                local_session_id
            };
            drop(sessions);
            state
                .bind_tab_state_to_session(&tab_id, &local_session_id)
                .await;
            state
                .append_history(&local_session_id, outcome.messages)
                .await;

            emit_agent_complete(
                &window,
                &tab_id,
                if outcome.suspended {
                    "suspended"
                } else {
                    "completed"
                },
            );
            Ok(local_session_id)
        }
        Err(message) => {
            state.store_pending_turn(pending_for_retry).await;
            if message == AGENT_CANCELLED_MESSAGE {
                emit_agent_complete(&window, &tab_id, "cancelled");
                return Err(message);
            }
            let error_message = message.clone();
            emit_agent_event(
                &window,
                &tab_id,
                AgentEventPayload::Error(AgentErrorEvent {
                    code: "agent_turn_resumed_failed".to_string(),
                    message: error_message,
                }),
            );
            emit_agent_complete(&window, &tab_id, "error");
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn agent_reset_tool_approvals(
    app: tauri::AppHandle,
    state: State<'_, AgentRuntimeState>,
    tab_id: String,
) -> Result<(), String> {
    state.ensure_storage(&app).await?;
    state.clear_tool_approvals(&tab_id).await;
    state.clear_pending_turn(&tab_id, None).await;
    Ok(())
}

#[tauri::command]
pub async fn agent_list_sessions(
    app: tauri::AppHandle,
    state: State<'_, AgentRuntimeState>,
    project_path: String,
) -> Result<Vec<AgentSessionSummary>, String> {
    state.ensure_storage(&app).await?;
    Ok(state
        .list_session_summaries_for_project(&project_path)
        .await)
}

#[tauri::command]
pub async fn agent_get_session_summary(
    app: tauri::AppHandle,
    state: State<'_, AgentRuntimeState>,
    local_session_id: String,
) -> Result<Option<AgentSessionSummary>, String> {
    state.ensure_storage(&app).await?;
    Ok(state.session_summary(&local_session_id).await)
}

#[tauri::command]
pub async fn agent_load_session_history(
    app: tauri::AppHandle,
    state: State<'_, AgentRuntimeState>,
    local_session_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    state.ensure_storage(&app).await?;
    let exists = {
        let sessions = state.sessions.lock().await;
        sessions.contains_key(&local_session_id)
    };

    if exists {
        Ok(state
            .history_for_session(&local_session_id)
            .await
            .unwrap_or_default())
    } else {
        Err(format!("Unknown local agent session: {}", local_session_id))
    }
}

#[tauri::command]
pub async fn agent_smoke_test(
    app: tauri::AppHandle,
    project_path: String,
) -> Result<AgentSmokeResult, String> {
    let runtime = selected_provider(&app, Some(&project_path))?;
    match runtime.provider.as_str() {
        "openai" => Err("OpenAI smoke test is not wired yet; current smoke harness targets chat-completions-class providers first.".to_string()),
        "minimax" | "deepseek" => {
            chat_completions::smoke_test(&app, &project_path, &runtime.provider).await
        }
        other => Err(format!("Unsupported agent provider in settings: {}.", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::AgentProvider;

    #[test]
    fn openai_provider_reports_env_or_streaming_mode() {
        let status = openai_provider().check_status();
        assert_eq!(status.provider, "openai");
        assert!(matches!(
            status.mode.as_str(),
            "env_missing" | "text_streaming_ready"
        ));
        assert_eq!(status.default_model.as_deref(), Some("gpt-5.4"));
    }

    #[tokio::test]
    async fn runtime_state_filters_sessions_by_project() {
        let state = AgentRuntimeState::default();
        let mut sessions = state.sessions.lock().await;
        let a = AgentSessionRecord::new(
            "openai",
            "/tmp/project-a".to_string(),
            "tab-a".to_string(),
            "Chat A".to_string(),
            "gpt-5.4".to_string(),
        );
        let b = AgentSessionRecord::new(
            "openai",
            "/tmp/project-b".to_string(),
            "tab-b".to_string(),
            "Chat B".to_string(),
            "gpt-5.4".to_string(),
        );
        sessions.insert(a.local_session_id.clone(), a.clone());
        sessions.insert(b.local_session_id.clone(), b);
        drop(sessions);

        let filtered = state.list_sessions_for_project("/tmp/project-a").await;
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].project_path, "/tmp/project-a");
        assert_eq!(filtered[0].tab_id, "tab-a");
    }
}
