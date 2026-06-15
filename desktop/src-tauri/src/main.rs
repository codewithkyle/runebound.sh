#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod commands;
mod entities;
mod repositories;
mod router;
mod services;
mod utils;

use std::path::PathBuf;
use std::sync::Arc;

use dnd_core::command::{CommandClientEvent, CommandResponse};
use dnd_core::command_manifest::CommandManifest;
use dnd_core::command_parse::{normalize_command_input, parse_command_input};
use dnd_core::db;
use tokio::sync::Mutex;

use crate::app_state::{AppState, EditorSession};
use crate::entities::build_default_registry;
use crate::repositories::{
    DocumentRepository, FactionRepository, GenerationRepository, ItemRepository, LocationRepository,
    NpcRepository, ProdDocumentRepository, ProdFactionRepository, ProdGenerationRepository,
    ProdItemRepository, ProdLocationRepository, ProdNpcRepository, ProdSoftDeleteRepository,
    ProdVaultRepository, SoftDeleteRepository, VaultRepository,
};
use crate::services::suggestions::{CommandSuggestion, SuggestionService};
use crate::services::vault_sync::VaultSyncService;

#[tauri::command]
async fn suggest_command_input(
    input: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CommandSuggestion>, String> {
    let service = SuggestionService;
    service.build_suggestions(input, state.inner()).await
}

#[tauri::command]
async fn run_command(
    input: String,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<CommandResponse, String> {
    let normalized_input = normalize_command_input(&input);
    let parsed = parse_command_input(&normalized_input);
    if !parsed.valid {
        let has_unknown_command = parsed
            .diagnostics
            .iter()
            .any(|diag| diag.code == "unknown_command");

        if !has_unknown_command {
            if let Some(diag) = parsed.diagnostics.first() {
                return Err(diag.message.clone());
            }
            return Err("invalid command".to_string());
        }
    }

    let onboarding_active = {
        let service = state.command_service.lock().await;
        service.session().onboarding.active
    };

    if onboarding_active {
        let mut service = state.command_service.lock().await;
        return Ok(service.execute_line(&normalized_input).await);
    }

    if let Some(response) =
        router::dispatch_desktop_command(&normalized_input, &parsed.normalized_tokens, state.clone(), app_handle.clone())
            .await?
    {
        let skip_history_push = matches!(
            response.client_event,
            Some(CommandClientEvent::ClearTerminal {
                clear_history: true
            })
        );
        if !skip_history_push {
            let trimmed = normalized_input.trim();
            if !trimmed.is_empty() {
                let mut service = state.command_service.lock().await;
                service.session_mut().push_history(trimmed, 50);
            }
        }
        return Ok(response);
    }

    let mut service = state.command_service.lock().await;
    Ok(service.execute_line(&normalized_input).await)
}

#[tauri::command]
fn get_command_manifest() -> CommandManifest {
    dnd_core::command_manifest::command_manifest()
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn main() {
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let database = tauri::async_runtime::block_on(db::init_database())
        .expect("failed to initialize sqlite database");
    let database = Arc::new(database);

    let vault_repo: Arc<dyn VaultRepository> = Arc::new(ProdVaultRepository);
    let npc_repo: Arc<dyn NpcRepository> = Arc::new(ProdNpcRepository);
    let location_repo: Arc<dyn LocationRepository> = Arc::new(ProdLocationRepository);
    let faction_repo: Arc<dyn FactionRepository> = Arc::new(ProdFactionRepository);
    let item_repo: Arc<dyn ItemRepository> = Arc::new(ProdItemRepository);
    let document_repo: Arc<dyn DocumentRepository> = Arc::new(ProdDocumentRepository);
    let generation_repo: Arc<dyn GenerationRepository> = Arc::new(ProdGenerationRepository);
    let soft_delete_repo: Arc<dyn SoftDeleteRepository> = Arc::new(ProdSoftDeleteRepository);

    let command_service = dnd_core::service::CommandService::new(workspace_root.clone());

    let domains = Arc::new(build_default_registry());

    let app_state = AppState {
        workspace_root,
        command_service: Mutex::new(command_service),
        editor_session: Mutex::new(EditorSession::default()),
        database: database.clone(),
        vault_repo: vault_repo.clone(),
        npc_repo: npc_repo.clone(),
        location_repo: location_repo.clone(),
        faction_repo: faction_repo.clone(),
        item_repo: item_repo.clone(),
        document_repo: document_repo.clone(),
        generation_repo: generation_repo.clone(),
        soft_delete_repo: soft_delete_repo.clone(),
        domains,
    };

    let vault_sync_service = VaultSyncService;
    if let Err(err) = tauri::async_runtime::block_on(vault_sync_service.sync_from_vault(&app_state)) {
        eprintln!("startup vault sync skipped: {err}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            run_command,
            suggest_command_input,
            get_command_manifest,
            exit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
