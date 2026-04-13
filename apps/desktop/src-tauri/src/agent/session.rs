pub use agent_core::session::*;

use tauri::Manager;

/// Tauri-specific helper: resolves the app config dir from AppHandle and delegates
/// to `AgentRuntimeState::ensure_storage_at`.
pub async fn ensure_storage_from_app(
    state: &AgentRuntimeState,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    let app_config_dir = app
        .path()
        .app_config_dir()
        .map_err(|err| format!("Failed to resolve app config dir: {}", err))?;
    state.ensure_storage_at(app_config_dir).await
}
