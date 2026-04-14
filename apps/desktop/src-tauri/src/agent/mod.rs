mod adapter;
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
mod workflows;

use agent_core::{
    build_agent_instructions_with_work_state, max_rounds_for_task, resolve_turn_profile,
    summarize_objective, tool_choice_for_task,
};
use tauri::{Manager, State, WebviewWindow};
use tokio::sync::watch;

pub use events::{
    AGENT_COMPLETE_EVENT_NAME, AGENT_EVENT_NAME, AgentCompletePayload, AgentErrorEvent,
    AgentEventEnvelope, AgentEventPayload, AgentStatusEvent,
};
pub use provider::{
    AgentResponseMode, AgentSamplingProfile, AgentSelectionScope, AgentStatus, AgentTaskKind,
    AgentTurnDescriptor, AgentTurnProfile,
};
pub use session::{
    AgentRuntimeState, AgentSessionRecord, AgentSessionSummary, AgentSessionWorkState,
    CollectedReference,
};
use turn_engine::{emit_tool_resumed, emit_turn_resumed};
use turn_engine::{
    emit_workflow_checkpoint_approved, emit_workflow_checkpoint_rejected,
    emit_workflow_checkpoint_requested,
};
use workflows::{AgentWorkflowState, AgentWorkflowType, WorkflowCheckpointDecision};

use crate::settings;

const AGENT_CANCELLED_MESSAGE: &str = "Agent run cancelled by user.";

#[allow(dead_code)]
pub fn build_agent_instructions(request: &AgentTurnDescriptor) -> String {
    build_agent_instructions_with_work_state(request, None, None, None)
}

pub async fn agent_instructions_for_request(
    state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    runtime_config: Option<&settings::AgentRuntimeConfig>,
) -> String {
    let work_state = state
        .work_state_for_prompt(&request.tab_id, request.local_session_id.as_deref())
        .await;
    let memory_context = state.build_memory_context().await;
    let mem_ref = if memory_context.is_empty() {
        None
    } else {
        Some(memory_context.as_str())
    };
    build_agent_instructions_with_work_state(request, Some(&work_state), runtime_config, mem_ref)
}

fn emit_agent_event(sink: &dyn agent_core::EventSink, tab_id: &str, payload: AgentEventPayload) {
    sink.emit_event(&AgentEventEnvelope {
        tab_id: tab_id.to_string(),
        payload,
    });
}

fn emit_agent_complete(sink: &dyn agent_core::EventSink, tab_id: &str, outcome: &str) {
    agent_core::emit_agent_complete(sink, tab_id, outcome);
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
    let sink = adapter::TauriEventSink { window };
    let runtime = selected_provider(&window.app_handle(), Some(&request.project_path))?;
    match runtime.provider.as_str() {
        "openai" => openai::run_turn_loop(&sink, window, state, request, cancel_rx).await,
        "minimax" | "deepseek" => {
            chat_completions::run_turn_loop(&sink, window, state, request, history, cancel_rx).await
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

async fn ensure_no_pending_workflow_checkpoint(
    state: &AgentRuntimeState,
    tab_id: &str,
    local_session_id: Option<&str>,
) -> Result<(), String> {
    if state
        .workflow_has_pending_checkpoint(tab_id, local_session_id)
        .await
    {
        return Err(
            "Workflow checkpoint is pending. Approve or request changes before continuing."
                .to_string(),
        );
    }
    Ok(())
}

async fn persist_turn_outcome(
    state: &AgentRuntimeState,
    runtime: &settings::AgentRuntimeConfig,
    request: &AgentTurnDescriptor,
    outcome: openai::AgentTurnOutcome,
) -> String {
    let selected_model = request
        .model
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| runtime.model.clone());

    let mut sessions = state.sessions.lock().await;
    let local_session_id = if let Some(local_session_id) = request.local_session_id.as_ref() {
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
        .bind_tab_state_to_session(&request.tab_id, &local_session_id)
        .await;
    state
        .append_history(&local_session_id, outcome.messages)
        .await;
    local_session_id
}

async fn run_checkpointed_workflow_turn(
    window: &WebviewWindow,
    state: &AgentRuntimeState,
    runtime: &settings::AgentRuntimeConfig,
    request: AgentTurnDescriptor,
    mut workflow: AgentWorkflowState,
    workflow_type_key: &str,
    task_kind: AgentTaskKind,
) -> Result<String, String> {
    let sink = adapter::TauriEventSink { window };
    workflow.can_run_stage()?;
    record_request_objective(
        state,
        &request.tab_id,
        request.local_session_id.as_deref(),
        &request.prompt,
    )
    .await;

    let staged_prompt = workflow.build_stage_prompt(&request.prompt);
    let mut staged_profile = resolve_turn_profile(&request);
    staged_profile.task_kind = task_kind;
    let staged_request = AgentTurnDescriptor {
        prompt: staged_prompt,
        turn_profile: Some(staged_profile),
        ..request.clone()
    };

    emit_agent_event(
        &sink,
        &request.tab_id,
        AgentEventPayload::Status(AgentStatusEvent {
            stage: "workflow_stage_running".to_string(),
            message: format!("Running workflow stage: {}...", workflow.stage_label()),
        }),
    );

    let prior_history = if let Some(local_session_id) = staged_request.local_session_id.as_ref() {
        state
            .history_for_session(local_session_id)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let cancel_rx = state.register_cancellation(&request.tab_id).await;
    let outcome = dispatch_run_turn_loop(
        window,
        state,
        &staged_request,
        &prior_history,
        Some(cancel_rx),
    )
    .await;
    state.clear_cancellation(&request.tab_id).await;

    match outcome {
        Ok(outcome) => {
            let suspended = outcome.suspended;
            let local_session_id = persist_turn_outcome(state, runtime, &request, outcome).await;
            workflow.bind_local_session_id(Some(&local_session_id));
            workflow.tab_id = request.tab_id.clone();

            if suspended {
                state.upsert_workflow_state(workflow).await;
                emit_agent_complete(&sink, &request.tab_id, "suspended");
                return Ok(local_session_id);
            }

            workflow.mark_stage_completed(&request.prompt);
            let stage = workflow.current_stage.clone();
            let message = format!(
                "Workflow stage '{}' completed. Approve the checkpoint to continue.",
                workflow.stage_label()
            );
            state.upsert_workflow_state(workflow).await;
            state
                .mark_pending_state(
                    &request.tab_id,
                    Some(&local_session_id),
                    "workflow_checkpoint",
                    workflow_type_key,
                    None,
                )
                .await;
            emit_workflow_checkpoint_requested(
                &sink,
                &request.tab_id,
                workflow_type_key,
                &stage,
                &message,
            );
            emit_agent_event(
                &sink,
                &request.tab_id,
                AgentEventPayload::Status(AgentStatusEvent {
                    stage: "workflow_checkpoint_requested".to_string(),
                    message: message.clone(),
                }),
            );
            emit_agent_complete(&sink, &request.tab_id, "suspended");
            Ok(local_session_id)
        }
        Err(message) => {
            if message == AGENT_CANCELLED_MESSAGE {
                emit_agent_complete(&sink, &request.tab_id, "cancelled");
                return Err(message);
            }
            let error_message = message.clone();
            emit_agent_event(
                &sink,
                &request.tab_id,
                AgentEventPayload::Error(AgentErrorEvent {
                    code: "workflow_stage_failed".to_string(),
                    message: error_message,
                }),
            );
            emit_agent_complete(&sink, &request.tab_id, "error");
            Err(message)
        }
    }
}

async fn run_paper_drafting_workflow_turn(
    window: &WebviewWindow,
    state: &AgentRuntimeState,
    runtime: &settings::AgentRuntimeConfig,
    request: AgentTurnDescriptor,
    workflow: AgentWorkflowState,
) -> Result<String, String> {
    run_checkpointed_workflow_turn(
        window,
        state,
        runtime,
        request,
        workflow,
        AgentWorkflowType::PaperDrafting.as_str(),
        AgentTaskKind::PaperDrafting,
    )
    .await
}

async fn run_literature_review_workflow_turn(
    window: &WebviewWindow,
    state: &AgentRuntimeState,
    runtime: &settings::AgentRuntimeConfig,
    request: AgentTurnDescriptor,
    workflow: AgentWorkflowState,
) -> Result<String, String> {
    run_checkpointed_workflow_turn(
        window,
        state,
        runtime,
        request,
        workflow,
        AgentWorkflowType::LiteratureReview.as_str(),
        AgentTaskKind::LiteratureReview,
    )
    .await
}

async fn run_peer_review_workflow_turn(
    window: &WebviewWindow,
    state: &AgentRuntimeState,
    runtime: &settings::AgentRuntimeConfig,
    request: AgentTurnDescriptor,
    workflow: AgentWorkflowState,
) -> Result<String, String> {
    run_checkpointed_workflow_turn(
        window,
        state,
        runtime,
        request,
        workflow,
        AgentWorkflowType::PeerReview.as_str(),
        AgentTaskKind::PeerReview,
    )
    .await
}

#[cfg(test)]
mod prompt_tests {
    use super::{build_agent_instructions, resolve_turn_profile};
    use crate::agent::provider::{
        AgentSelectionScope, AgentTaskKind, AgentTurnDescriptor, AgentTurnProfile,
    };

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
    fn build_agent_instructions_wrapper_returns_non_empty_prompt_guidance() {
        let request = make_request("Summarize the attached evidence.", None);
        let instructions = build_agent_instructions(&request);
        assert!(!instructions.trim().is_empty());
        assert!(instructions.contains("[Hard execution rules]"));
    }

    #[test]
    fn imported_resolve_turn_profile_classifies_simple_edit_request() {
        let request = make_request(
            "[Currently open file: main.tex]\n[Selection: @main.tex:1:1-1:4]\n[Selected text:\ntext\n]\n\nrefine this paragraph",
            None,
        );
        let profile = resolve_turn_profile(&request);
        assert_eq!(profile.task_kind, AgentTaskKind::SelectionEdit);
        assert_eq!(profile.selection_scope, AgentSelectionScope::SelectedSpan);
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
    session::ensure_storage_from_app(&state, &window.app_handle()).await?;
    let sink = adapter::TauriEventSink { window: &window };
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
    ensure_no_pending_workflow_checkpoint(&state, &tab_id, None).await?;
    state.acquire_turn_guard(&tab_id).await?;

    emit_agent_event(
        &sink,
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
                &sink,
                &tab_id,
                if outcome.suspended {
                    "suspended"
                } else {
                    "completed"
                },
            );
            state.release_turn_guard(&tab_id).await;
            Ok(local_session_id)
        }
        Err(message) => {
            state.release_turn_guard(&tab_id).await;
            if message == AGENT_CANCELLED_MESSAGE {
                emit_agent_complete(&sink, &tab_id, "cancelled");
                return Err(message);
            }
            let error_message = message.clone();
            emit_agent_event(
                &sink,
                &tab_id,
                AgentEventPayload::Error(AgentErrorEvent {
                    code: "agent_turn_failed".to_string(),
                    message: error_message,
                }),
            );
            emit_agent_complete(&sink, &tab_id, "error");
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
    session::ensure_storage_from_app(&state, &window.app_handle()).await?;
    let sink = adapter::TauriEventSink { window: &window };
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
    ensure_no_pending_workflow_checkpoint(&state, &tab_id, request.local_session_id.as_deref())
        .await?;
    state.acquire_turn_guard(&tab_id).await?;

    emit_agent_event(
        &sink,
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
                &sink,
                &tab_id,
                if outcome.suspended {
                    "suspended"
                } else {
                    "completed"
                },
            );
            state.release_turn_guard(&tab_id).await;
            Ok(local_session_id)
        }
        Err(message) => {
            state.release_turn_guard(&tab_id).await;
            if message == AGENT_CANCELLED_MESSAGE {
                emit_agent_complete(&sink, &tab_id, "cancelled");
                return Err(message);
            }
            let error_message = message.clone();
            emit_agent_event(
                &sink,
                &tab_id,
                AgentEventPayload::Error(AgentErrorEvent {
                    code: "agent_turn_failed".to_string(),
                    message: error_message,
                }),
            );
            emit_agent_complete(&sink, &tab_id, "error");
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn agent_start_workflow(
    window: WebviewWindow,
    state: State<'_, AgentRuntimeState>,
    project_path: String,
    prompt: String,
    tab_id: String,
    model: Option<String>,
    workflow_type: Option<String>,
    turn_profile: Option<AgentTurnProfile>,
) -> Result<String, String> {
    session::ensure_storage_from_app(&state, &window.app_handle()).await?;
    let sink = adapter::TauriEventSink { window: &window };
    let runtime = selected_provider(&window.app_handle(), Some(&project_path))?;
    let workflow_kind = workflow_type
        .unwrap_or_else(|| "paper_drafting".to_string())
        .trim()
        .to_ascii_lowercase();
    let workflow_type = match workflow_kind.as_str() {
        "paper_drafting" => AgentWorkflowType::PaperDrafting,
        "literature_review" => AgentWorkflowType::LiteratureReview,
        "peer_review" => AgentWorkflowType::PeerReview,
        _ => return Err(format!("Unsupported workflow type: {}", workflow_kind)),
    };

    state.clear_workflow_state(&tab_id, None).await;
    state.acquire_turn_guard(&tab_id).await?;
    let workflow = match workflow_type {
        AgentWorkflowType::PaperDrafting => {
            AgentWorkflowState::new_paper_drafting(&tab_id, &project_path, model.clone())
        }
        AgentWorkflowType::LiteratureReview => {
            AgentWorkflowState::new_literature_review(&tab_id, &project_path, model.clone())
        }
        AgentWorkflowType::PeerReview => {
            AgentWorkflowState::new_peer_review(&tab_id, &project_path, model.clone())
        }
    };
    state.upsert_workflow_state(workflow.clone()).await;

    emit_agent_event(
        &sink,
        &tab_id,
        AgentEventPayload::Status(AgentStatusEvent {
            stage: "workflow_started".to_string(),
            message: format!("Started {} workflow.", workflow_type.as_str()),
        }),
    );

    let request = AgentTurnDescriptor {
        project_path,
        prompt,
        tab_id: tab_id.clone(),
        model,
        local_session_id: None,
        previous_response_id: None,
        turn_profile,
    };
    let result = match workflow_type {
        AgentWorkflowType::PaperDrafting => {
            run_paper_drafting_workflow_turn(&window, &state, &runtime, request, workflow).await
        }
        AgentWorkflowType::LiteratureReview => {
            run_literature_review_workflow_turn(&window, &state, &runtime, request, workflow).await
        }
        AgentWorkflowType::PeerReview => {
            run_peer_review_workflow_turn(&window, &state, &runtime, request, workflow).await
        }
    };
    state.release_turn_guard(&tab_id).await;
    result
}

#[tauri::command]
pub async fn agent_continue_workflow(
    window: WebviewWindow,
    state: State<'_, AgentRuntimeState>,
    project_path: String,
    prompt: String,
    tab_id: String,
    model: Option<String>,
    local_session_id: Option<String>,
    turn_profile: Option<AgentTurnProfile>,
) -> Result<String, String> {
    session::ensure_storage_from_app(&state, &window.app_handle()).await?;
    let runtime = selected_provider(&window.app_handle(), Some(&project_path))?;
    let Some(mut workflow) = state
        .workflow_state_for(&tab_id, local_session_id.as_deref())
        .await
    else {
        return Err("No active workflow found for this tab/session.".to_string());
    };
    let resolved_local_session_id = local_session_id.or(workflow.local_session_id.clone());
    workflow.tab_id = tab_id.clone();
    workflow.bind_local_session_id(resolved_local_session_id.as_deref());
    state.upsert_workflow_state(workflow.clone()).await;
    state.acquire_turn_guard(&tab_id).await?;

    let request = AgentTurnDescriptor {
        project_path,
        prompt,
        tab_id: tab_id.clone(),
        model,
        local_session_id: resolved_local_session_id,
        previous_response_id: None,
        turn_profile,
    };
    let result = match workflow.workflow_type {
        AgentWorkflowType::PaperDrafting => {
            run_paper_drafting_workflow_turn(&window, &state, &runtime, request, workflow).await
        }
        AgentWorkflowType::LiteratureReview => {
            run_literature_review_workflow_turn(&window, &state, &runtime, request, workflow).await
        }
        AgentWorkflowType::PeerReview => {
            run_peer_review_workflow_turn(&window, &state, &runtime, request, workflow).await
        }
    };
    state.release_turn_guard(&tab_id).await;
    result
}

#[tauri::command]
pub async fn agent_checkpoint_action(
    window: WebviewWindow,
    state: State<'_, AgentRuntimeState>,
    tab_id: String,
    local_session_id: Option<String>,
    decision: String,
    feedback: Option<String>,
) -> Result<(), String> {
    session::ensure_storage_from_app(&state, &window.app_handle()).await?;
    let sink = adapter::TauriEventSink { window: &window };
    let Some(mut workflow) = state
        .workflow_state_for(&tab_id, local_session_id.as_deref())
        .await
    else {
        return Err("No active workflow found for this tab/session.".to_string());
    };
    let Some(decision) = WorkflowCheckpointDecision::from_str(&decision) else {
        return Err("Unsupported checkpoint decision. Use approve or request_changes.".to_string());
    };

    let transition = workflow.apply_checkpoint_decision(decision.clone())?;
    state
        .clear_pending_state(&tab_id, local_session_id.as_deref())
        .await;

    match decision {
        WorkflowCheckpointDecision::ApproveStage => {
            if transition.completed {
                state
                    .clear_workflow_state(&tab_id, local_session_id.as_deref())
                    .await;
            } else {
                state.upsert_workflow_state(workflow).await;
            }
            let message = if transition.completed {
                "Workflow completed. You can start a new workflow anytime.".to_string()
            } else {
                format!("Checkpoint approved. Next stage: {}.", transition.to_stage)
            };
            emit_workflow_checkpoint_approved(
                &sink,
                &tab_id,
                transition.workflow_type.as_str(),
                &transition.from_stage,
                &transition.to_stage,
                transition.completed,
                &message,
            );
            emit_agent_event(
                &sink,
                &tab_id,
                AgentEventPayload::Status(AgentStatusEvent {
                    stage: if transition.completed {
                        "workflow_completed".to_string()
                    } else {
                        "workflow_checkpoint_approved".to_string()
                    },
                    message,
                }),
            );
        }
        WorkflowCheckpointDecision::RequestChanges => {
            state.upsert_workflow_state(workflow).await;
            let message = feedback
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| {
                    "Checkpoint rejected. Revise this stage before continuing.".to_string()
                });
            emit_workflow_checkpoint_rejected(
                &sink,
                &tab_id,
                transition.workflow_type.as_str(),
                &transition.to_stage,
                &message,
            );
            emit_agent_event(
                &sink,
                &tab_id,
                AgentEventPayload::Status(AgentStatusEvent {
                    stage: "workflow_checkpoint_rejected".to_string(),
                    message,
                }),
            );
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn agent_cancel_turn(
    window: WebviewWindow,
    state: State<'_, AgentRuntimeState>,
    tab_id: String,
    response_id: Option<String>,
) -> Result<(), String> {
    let sink = adapter::TauriEventSink { window: &window };
    if let Err(message) = dispatch_cancel_turn(
        &window.app_handle(),
        &state,
        &tab_id,
        response_id.as_deref(),
    )
    .await
    {
        emit_agent_event(
            &sink,
            &tab_id,
            AgentEventPayload::Error(AgentErrorEvent {
                code: "agent_cancel_failed".to_string(),
                message,
            }),
        );
    }

    emit_agent_complete(&sink, &tab_id, "cancelled");
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
    session::ensure_storage_from_app(&state, &app).await?;
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
    session::ensure_storage_from_app(&state, &window.app_handle()).await?;
    let sink = adapter::TauriEventSink { window: &window };
    let Some(pending) = state.take_pending_turn(&tab_id).await else {
        return Err("No pending approved turn to resume.".to_string());
    };
    let pending_for_retry = pending.clone();
    state.acquire_turn_guard(&tab_id).await?;

    emit_turn_resumed(
        &sink,
        &tab_id,
        pending.local_session_id.as_deref(),
        "Resuming the suspended turn after approval.",
    );
    emit_tool_resumed(
        &sink,
        &tab_id,
        &pending.approval_tool_name,
        pending.target_label.as_deref(),
        "Approved tool is resuming in the current turn.",
    );
    emit_agent_event(
        &sink,
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
                &sink,
                &tab_id,
                if outcome.suspended {
                    "suspended"
                } else {
                    "completed"
                },
            );
            state.release_turn_guard(&tab_id).await;
            Ok(local_session_id)
        }
        Err(message) => {
            state.release_turn_guard(&tab_id).await;
            state.store_pending_turn(pending_for_retry).await;
            if message == AGENT_CANCELLED_MESSAGE {
                emit_agent_complete(&sink, &tab_id, "cancelled");
                return Err(message);
            }
            let error_message = message.clone();
            emit_agent_event(
                &sink,
                &tab_id,
                AgentEventPayload::Error(AgentErrorEvent {
                    code: "agent_turn_resumed_failed".to_string(),
                    message: error_message,
                }),
            );
            emit_agent_complete(&sink, &tab_id, "error");
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
    session::ensure_storage_from_app(&state, &app).await?;
    state.clear_tool_approvals(&tab_id).await;
    state.clear_pending_turn(&tab_id, None).await;
    state.clear_workflow_state(&tab_id, None).await;
    Ok(())
}

#[tauri::command]
pub async fn agent_list_sessions(
    app: tauri::AppHandle,
    state: State<'_, AgentRuntimeState>,
    project_path: String,
) -> Result<Vec<AgentSessionSummary>, String> {
    session::ensure_storage_from_app(&state, &app).await?;
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
    session::ensure_storage_from_app(&state, &app).await?;
    Ok(state.session_summary(&local_session_id).await)
}

#[tauri::command]
pub async fn agent_load_session_history(
    app: tauri::AppHandle,
    state: State<'_, AgentRuntimeState>,
    local_session_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    session::ensure_storage_from_app(&state, &app).await?;
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
pub async fn agent_get_collected_references(
    app: tauri::AppHandle,
    state: State<'_, AgentRuntimeState>,
    tab_id: String,
    local_session_id: Option<String>,
) -> Result<Vec<CollectedReference>, String> {
    session::ensure_storage_from_app(&state, &app).await?;
    Ok(state
        .collected_references_for(&tab_id, local_session_id.as_deref())
        .await)
}

#[tauri::command]
pub async fn agent_update_collected_reference(
    app: tauri::AppHandle,
    state: State<'_, AgentRuntimeState>,
    tab_id: String,
    local_session_id: Option<String>,
    doi: Option<String>,
    pmid: Option<String>,
    title: Option<String>,
    user_notes: Option<String>,
    relevance_tag: Option<String>,
) -> Result<(), String> {
    session::ensure_storage_from_app(&state, &app).await?;
    state
        .update_collected_reference(
            &tab_id,
            local_session_id.as_deref(),
            doi,
            pmid,
            title,
            user_notes,
            relevance_tag,
        )
        .await
}

#[tauri::command]
pub async fn agent_clear_collected_references(
    app: tauri::AppHandle,
    state: State<'_, AgentRuntimeState>,
    tab_id: String,
    local_session_id: Option<String>,
) -> Result<(), String> {
    session::ensure_storage_from_app(&state, &app).await?;
    state
        .clear_collected_references(&tab_id, local_session_id.as_deref())
        .await;
    Ok(())
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
