use reqwest::Client;
use std::sync::Arc;
use tokio::sync::watch;

pub use agent_core::AgentTurnOutcome;
use agent_core::{ConfigProvider, EventSink, ToolExecutorFn};

use super::adapter::TauriConfigProvider;
use super::provider::{AgentProvider, AgentStatus, AgentTurnDescriptor, AgentTurnHandle};
use super::session::AgentRuntimeState;
use super::tools::execute_tool_call;

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, Default, Clone)]
pub struct OpenAiProvider;

impl OpenAiProvider {
    #[allow(dead_code)]
    fn skeleton_error() -> String {
        "OpenAI agent provider skeleton is in place, but the Responses API transport is not wired yet."
            .to_string()
    }

    fn from_env() -> Result<agent_core::openai::OpenAiConfig, String> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| "OPENAI_API_KEY is not set".to_string())?
            .trim()
            .to_string();

        if api_key.is_empty() {
            return Err("OPENAI_API_KEY is empty".to_string());
        }

        let base_url = std::env::var("OPENAI_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        let default_model = std::env::var("OPENAI_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "gpt-5.4".to_string());

        Ok(agent_core::openai::OpenAiConfig {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            default_model,
            source: "env".to_string(),
            sampling_profiles: None,
        })
    }
}

impl AgentProvider for OpenAiProvider {
    fn provider_id(&self) -> &'static str {
        "openai"
    }

    fn display_name(&self) -> &'static str {
        "OpenAI Responses"
    }

    fn default_model(&self) -> Option<&'static str> {
        Some("gpt-5.4")
    }

    fn check_status(&self) -> AgentStatus {
        match Self::from_env() {
            Ok(config) => AgentStatus {
                provider: self.provider_id().to_string(),
                display_name: self.display_name().to_string(),
                ready: true,
                mode: "text_streaming_ready".to_string(),
                message: "OpenAI Responses text streaming is configured via environment variables."
                    .to_string(),
                default_model: Some(config.default_model),
            },
            Err(message) => AgentStatus {
                provider: self.provider_id().to_string(),
                display_name: self.display_name().to_string(),
                ready: false,
                mode: "env_missing".to_string(),
                message,
                default_model: self.default_model().map(str::to_string),
            },
        }
    }

    fn start_turn(&self, _request: &AgentTurnDescriptor) -> Result<AgentTurnHandle, String> {
        Err(Self::skeleton_error())
    }

    fn continue_turn(&self, _request: &AgentTurnDescriptor) -> Result<AgentTurnHandle, String> {
        Err(Self::skeleton_error())
    }

    fn cancel_turn(&self, _response_id: &str) -> Result<(), String> {
        Err(Self::skeleton_error())
    }
}

pub fn runtime_status(app: &tauri::AppHandle) -> AgentStatus {
    let config_provider = TauriConfigProvider { app };
    match agent_core::openai::load_runtime_config(&config_provider, None) {
        Ok(config) => AgentStatus {
            provider: "openai".to_string(),
            display_name: "OpenAI Responses".to_string(),
            ready: true,
            mode: if config.source == "settings" {
                "settings_ready".to_string()
            } else {
                "env_fallback_ready".to_string()
            },
            message: if config.source == "settings" {
                "OpenAI Responses is configured through Settings.".to_string()
            } else {
                "OpenAI Responses is configured through environment variables.".to_string()
            },
            default_model: Some(config.default_model),
        },
        Err(message) => AgentStatus {
            provider: "openai".to_string(),
            display_name: "OpenAI Responses".to_string(),
            ready: false,
            mode: "not_configured".to_string(),
            message,
            default_model: Some("gpt-5.4".to_string()),
        },
    }
}

pub async fn run_turn_loop(
    sink: &dyn EventSink,
    config_provider: &dyn ConfigProvider,
    runtime_state: &AgentRuntimeState,
    request: &AgentTurnDescriptor,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<AgentTurnOutcome, String> {
    let executor_state = runtime_state.clone();
    let tab_id = request.tab_id.clone();
    let project_path = request.project_path.clone();
    let tool_executor: ToolExecutorFn = Arc::new(move |call, cancel_rx| {
        let runtime_state = executor_state.clone();
        let tab_id = tab_id.clone();
        let project_path = project_path.clone();
        Box::pin(async move {
            execute_tool_call(&runtime_state, &tab_id, &project_path, call, cancel_rx).await
        })
    });

    agent_core::openai::run_turn_loop(
        sink,
        config_provider,
        runtime_state,
        request,
        tool_executor,
        cancel_rx,
    )
    .await
}

pub async fn cancel_response(app: &tauri::AppHandle, response_id: &str) -> Result<(), String> {
    let config_provider = TauriConfigProvider { app };
    let config = agent_core::openai::load_runtime_config(&config_provider, None)?;
    let client = Client::new();
    let url = format!("{}/responses/{}/cancel", config.base_url, response_id);
    let response = client
        .post(url)
        .bearer_auth(config.api_key)
        .send()
        .await
        .map_err(|err| format!("OpenAI cancel request failed: {}", err))?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(format!(
            "OpenAI cancel request failed with status {}: {}",
            status, body
        ))
    }
}
