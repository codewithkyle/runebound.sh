#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod boot;
mod commands;
mod entities;
mod repositories;
mod router;
mod services;
mod utils;
mod wizards;

use std::sync::Arc;

use dnd_core::command::{CommandClientEvent, CommandResponse, reject_help_flags};
use dnd_core::command_manifest::CommandManifest;
use dnd_core::command_parse::{normalize_command_input, parse_command_input};
use dnd_core::db;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::app_state::{AppState, EditorSession};
use crate::entities::build_default_registry;
use crate::repositories::{
    DocumentRepository, DungeonRepository, EventRepository, FactionRepository,
    GenerationRepository, GodRepository, ItemRepository, LocationRepository, MonsterRepository,
    NpcRepository, ProdDocumentRepository, ProdDungeonRepository, ProdEventRepository,
    ProdFactionRepository, ProdGenerationRepository, ProdGodRepository, ProdItemRepository,
    ProdLocationRepository, ProdMonsterRepository, ProdNpcRepository, ProdSoftDeleteRepository,
    ProdSpellRepository, ProdVaultRepository, SoftDeleteRepository, SpellRepository,
    VaultRepository,
};
use crate::services::suggestions::{CommandSuggestion, SuggestionService};
use crate::wizards::build_default_wizard_registry;

#[tauri::command]
async fn suggest_command_input(
    input: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CommandSuggestion>, String> {
    let service = SuggestionService;
    service.build_suggestions(input, state.inner()).await
}

/// Cancellable wrapper around [`run_command_inner`]: it arms a per-command cancellation
/// token in `AppState`, runs the dispatch under `tokio::select!`, and lets a concurrent
/// [`cancel_generation`] abort it (CTRL+C during a slow LLM generation). When the token
/// fires, `select!` drops the dispatch future, which drops the in-flight reqwest request.
#[tauri::command]
async fn run_command(
    input: String,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<CommandResponse, String> {
    let cancel = CancellationToken::new();
    *state.active_cancel.lock().unwrap() = Some(cancel.clone());
    let outcome = tokio::select! {
        biased;
        () = cancel.cancelled() => Err("Generation cancelled.".to_string()),
        result = run_command_inner(input, state.clone(), app_handle) => result,
    };
    // This command has settled; disarm so a later CTRL+C can't fire a stale token. The
    // frontend serializes commands, so no other `run_command` can be in flight here.
    *state.active_cancel.lock().unwrap() = None;
    outcome
}

async fn run_command_inner(
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

    // Generic wizard dispatch: while a wizard is active, route the raw line to the
    // wizard engine before registry dispatch, so step answers like `"2"` or a
    // free-text premise aren't parsed as commands. This is the single, first-class
    // dispatch route for every multi-step wizard.
    if let Some(response) =
        crate::wizards::try_execute_active_wizard(&normalized_input, state.inner()).await?
    {
        let trimmed = normalized_input.trim();
        if !trimmed.is_empty() {
            let mut service = state.command_service.lock().await;
            service.session_mut().push_history(trimmed, 50);
        }
        return Ok(response);
    }

    // Onboarding entry commands (start setup / setup vault|llm|model / model) launch
    // their wizard on *this* host, so the native folder picker is available via
    // `AppState::perform_native`. Active-onboarding input is consumed by the generic
    // wizard route above; `setup verbosity`/`setup help` are not entry commands and
    // fall through to the core handlers.
    if let Some(id) = dnd_core::onboarding_wizard::onboarding_entry_wizard_id(&normalized_input)
        && let Some(response) = crate::wizards::start_wizard(id, state.inner()).await?
    {
        let trimmed = normalized_input.trim();
        if !trimmed.is_empty() {
            let mut service = state.command_service.lock().await;
            service.session_mut().push_history(trimmed, 50);
        }
        return Ok(response);
    }

    // Reject `-h`/`--help` uniformly for desktop dispatch and the core fallthrough,
    // mirroring core's own guard (onboarding above forwards to the guarded `execute_line`).
    if let Some(message) = reject_help_flags(&parsed.normalized_tokens) {
        return Err(message);
    }

    if let Some(response) = router::dispatch_desktop_command(
        &normalized_input,
        &parsed.normalized_tokens,
        state.clone(),
        app_handle.clone(),
    )
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

/// Abort the command currently in flight, if any, by firing its cancellation token.
/// Invoked by the frontend when the user presses CTRL+C during a generation. A no-op
/// when nothing is running (the token slot is empty). Runs concurrently with the pending
/// `run_command` because Tauri dispatches each `invoke` as its own task.
#[tauri::command]
async fn cancel_generation(state: tauri::State<'_, AppState>) -> Result<(), String> {
    if let Some(token) = state.active_cancel.lock().unwrap().as_ref() {
        token.cancel();
    }
    Ok(())
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
    // Backfill config sections added since the user's config was written (e.g.
    // `[generation]`) so they are visible and editable on disk. Best-effort.
    if let Err(err) = dnd_core::config::ensure_config_sections_persisted() {
        eprintln!("config migration warning: {err:#}");
    }

    let database = tauri::async_runtime::block_on(db::init_database())
        .expect("failed to initialize sqlite database");
    let database = Arc::new(database);

    let vault_repo: Arc<dyn VaultRepository> = Arc::new(ProdVaultRepository);
    let npc_repo: Arc<dyn NpcRepository> = Arc::new(ProdNpcRepository);
    let location_repo: Arc<dyn LocationRepository> = Arc::new(ProdLocationRepository);
    let faction_repo: Arc<dyn FactionRepository> = Arc::new(ProdFactionRepository);
    let item_repo: Arc<dyn ItemRepository> = Arc::new(ProdItemRepository);
    let event_repo: Arc<dyn EventRepository> = Arc::new(ProdEventRepository);
    let god_repo: Arc<dyn GodRepository> = Arc::new(ProdGodRepository);
    let dungeon_repo: Arc<dyn DungeonRepository> = Arc::new(ProdDungeonRepository);
    let document_repo: Arc<dyn DocumentRepository> = Arc::new(ProdDocumentRepository);
    let generation_repo: Arc<dyn GenerationRepository> = Arc::new(ProdGenerationRepository);
    let soft_delete_repo: Arc<dyn SoftDeleteRepository> = Arc::new(ProdSoftDeleteRepository);
    let spell_repo: Arc<dyn SpellRepository> = Arc::new(ProdSpellRepository);
    let monster_repo: Arc<dyn MonsterRepository> = Arc::new(ProdMonsterRepository);

    let command_service = dnd_core::service::CommandService::new();

    let domains = Arc::new(build_default_registry());
    let wizards = Arc::new(build_default_wizard_registry());

    let app_state = AppState {
        command_service: Mutex::new(command_service),
        editor_session: Mutex::new(EditorSession::default()),
        database: database.clone(),
        vault_repo: vault_repo.clone(),
        npc_repo: npc_repo.clone(),
        location_repo: location_repo.clone(),
        faction_repo: faction_repo.clone(),
        item_repo: item_repo.clone(),
        event_repo: event_repo.clone(),
        god_repo: god_repo.clone(),
        dungeon_repo: dungeon_repo.clone(),
        document_repo: document_repo.clone(),
        generation_repo: generation_repo.clone(),
        soft_delete_repo: soft_delete_repo.clone(),
        spell_repo: spell_repo.clone(),
        monster_repo: monster_repo.clone(),
        domains,
        wizards,
        wizard_session: Mutex::new(crate::wizards::WizardSession::default()),
        boot_ollama_health: Mutex::new(None),
        app_handle: std::sync::Mutex::new(None),
        active_cancel: std::sync::Mutex::new(None),
    };

    // Startup cleanup (vault sync / soft-delete reaping) now runs as the
    // `cleanup` boot task so the user sees a spinner for it.

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .setup(|app| {
            // Stash the app handle so the onboarding wizard's native folder picker
            // (AppState::perform_native) can reach the dialog plugin.
            use tauri::Manager;
            let state = app.state::<AppState>();
            *state.app_handle.lock().unwrap() = Some(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            run_command,
            cancel_generation,
            suggest_command_input,
            get_command_manifest,
            exit_app,
            boot::boot_plan,
            boot::run_boot_task,
            boot::boot_motd
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
