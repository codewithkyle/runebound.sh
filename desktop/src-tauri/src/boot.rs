//! Boot subsystem: a small ordered registry of startup tasks the frontend runs
//! one at a time, showing a spinner per task, before rendering the welcome/MOTD.
//!
//! Adding a future boot step is a single entry in [`boot_task_infos`] plus a
//! match arm in [`run_boot_task`].

use serde::Serialize;
use tauri::State;

use dnd_core::command::{OLLAMA_BOOT_TIMEOUT_SECONDS, render_motd};
use dnd_core::config::{load_effective, required_issues};
use dnd_core::health::check_ollama_health;
use runebound_models::CommandResponse;

use crate::app_state::AppState;
use crate::commands::ok_response_with_doc;
use crate::services::vault_sync::VaultSyncService;

#[derive(Debug, Clone, Serialize)]
pub struct BootTaskInfo {
    pub id: String,
    /// Fun, in-world label shown next to the spinner.
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BootPlan {
    /// When true the app is not configured yet; the frontend skips the spinners
    /// and shows the first-time setup message instead.
    pub needs_setup: bool,
    pub tasks: Vec<BootTaskInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BootTaskResult {
    pub ok: bool,
    /// Status tone for the finished spinner: "success" | "warning" | "error".
    pub tone: String,
    pub detail: String,
}

fn boot_task_infos() -> Vec<BootTaskInfo> {
    vec![
        BootTaskInfo {
            id: "cleanup".to_string(),
            label: "cleaning up owlbear droppings".to_string(),
        },
        BootTaskInfo {
            id: "llm".to_string(),
            label: "warming up the sending stones".to_string(),
        },
    ]
}

/// Return the ordered boot tasks, or signal that first-time setup is required.
#[tauri::command]
pub fn boot_plan(state: State<'_, AppState>) -> Result<BootPlan, String> {
    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    let needs_setup = !required_issues(&loaded.effective).is_empty();
    let tasks = if needs_setup {
        Vec::new()
    } else {
        boot_task_infos()
    };
    Ok(BootPlan { needs_setup, tasks })
}

/// Run a single boot task by id. The frontend enforces the minimum spinner time.
#[tauri::command]
pub async fn run_boot_task(
    id: String,
    state: State<'_, AppState>,
) -> Result<BootTaskResult, String> {
    match id.as_str() {
        "cleanup" => {
            // Migrations already ran at `init_database`; this finalizes pending
            // publishes and reaps soft-deleted entities / TOML via the vault sync.
            VaultSyncService.sync_from_vault(state.inner()).await?;
            Ok(BootTaskResult {
                ok: true,
                tone: "success".to_string(),
                detail: "vault and database are tidy".to_string(),
            })
        }
        "llm" => {
            let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
            let health =
                check_ollama_health(&loaded.effective, OLLAMA_BOOT_TIMEOUT_SECONDS).await;

            let tone = if health.reachable && health.model_available {
                "success"
            } else {
                "warning"
            };
            let result = BootTaskResult {
                ok: health.reachable && health.model_available,
                tone: tone.to_string(),
                detail: health.detail.clone(),
            };

            // Cache for the MOTD so we don't probe the server twice.
            *state.boot_ollama_health.lock().await = Some(health);
            Ok(result)
        }
        other => Err(format!("unknown boot task: {other}")),
    }
}

/// Render the welcome/MOTD output with accurate connection info, reusing the
/// cached boot probe result where available.
#[tauri::command]
pub async fn boot_motd(state: State<'_, AppState>) -> Result<CommandResponse, String> {
    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    let config = loaded.effective;

    let health = {
        let cached = state.boot_ollama_health.lock().await.clone();
        match cached {
            Some(health) => health,
            None => check_ollama_health(&config, OLLAMA_BOOT_TIMEOUT_SECONDS).await,
        }
    };

    let vault_root = config
        .vault
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();

    let output = render_motd(
        &vault_root,
        &config.ollama.base_url,
        config.ollama.model.as_deref(),
        &health,
    );

    Ok(ok_response_with_doc(output.output, output.output_doc, None))
}
