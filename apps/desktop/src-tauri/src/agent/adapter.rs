//! Tauri adapter — bridges agent-core traits with Tauri's WebviewWindow / AppHandle.

use std::path::PathBuf;

use tauri::{Manager, WebviewWindow};

use agent_core::{
    AgentCompletePayload, AgentEventEnvelope, AgentRuntimeConfig, ConfigProvider, EventSink,
    AGENT_COMPLETE_EVENT_NAME, AGENT_EVENT_NAME,
};

use crate::settings;

// ── TauriEventSink ─────────────────────────────────────────────────

/// Routes agent events to the Tauri WebviewWindow.
pub struct TauriEventSink<'w> {
    pub window: &'w WebviewWindow,
}

impl<'w> EventSink for TauriEventSink<'w> {
    fn emit_event(&self, envelope: &AgentEventEnvelope) {
        let _ = self.window.emit(AGENT_EVENT_NAME, envelope);
    }

    fn emit_complete(&self, payload: &AgentCompletePayload) {
        let _ = self.window.emit(AGENT_COMPLETE_EVENT_NAME, payload);
    }
}

// ── TauriConfigProvider ────────────────────────────────────────────

/// Loads agent configuration from the Tauri settings system.
pub struct TauriConfigProvider<'a> {
    pub app: &'a tauri::AppHandle,
}

impl<'a> ConfigProvider for TauriConfigProvider<'a> {
    fn load_agent_runtime(
        &self,
        project_root: Option<&str>,
    ) -> Result<AgentRuntimeConfig, String> {
        settings::load_agent_runtime(self.app, project_root)
    }

    fn app_config_dir(&self) -> Result<PathBuf, String> {
        self.app
            .path()
            .app_config_dir()
            .map_err(|err| format!("Failed to resolve app config dir: {}", err))
    }

    fn project_storage_dir(&self, project_root: &str) -> Result<PathBuf, String> {
        Ok(PathBuf::from(project_root).join(".chat-prism"))
    }
}
