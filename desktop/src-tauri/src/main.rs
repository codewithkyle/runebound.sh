#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod commands;
mod repositories;
mod router;
mod services;
mod utils;

use std::path::PathBuf;
use std::sync::Arc;

use dnd_core::command::{CommandClientEvent, CommandResponse};
use dnd_core::command_manifest::CommandManifest;
use dnd_core::command_parse::{normalize_command_input, parse_command_input};
use dnd_core::config::{load_effective, validate_for_runtime};
use dnd_core::db;
use dnd_core::npc::{
    LocationFrontmatter, UNKNOWN_LOCATION, make_entity_id, now_timestamp, render_location_markdown,
    slugify, unique_slug_for_dir,
};
use dnd_core::vault::Vault;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::app_state::{AppState, EditorSession};
use crate::repositories::{
    DocumentRepository, FactionRepository, GenerationRepository, LocationRepository, NpcRepository,
    ProdDocumentRepository, ProdFactionRepository, ProdGenerationRepository, ProdLocationRepository,
    ProdNpcRepository, ProdSoftDeleteRepository, ProdVaultRepository, SoftDeleteRepository,
    VaultRepository,
};
use crate::services::suggestions::{CommandSuggestion, SuggestionService};
use crate::services::vault_sync::{
    move_vault_file, unique_markdown_path_for_name, unique_trash_path, VaultSyncService,
};
use crate::utils::{
    EntityDetails, EntityType, EnsureLocationInput, EnsureLocationResult, SoftDeleteEntityInput,
    SoftDeleteEntityResult, UndoSoftDeleteResult, carrying_from_db_text, exports_from_db_text,
    faction_list_from_db_text,
    normalize_relative_path_for_storage,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NpcDeletePayload {
    id: String,
    slug: String,
    name: String,
    race: String,
    occupation: String,
    sex: String,
    age: String,
    height: String,
    weight_lbs: String,
    background: String,
    want_need: String,
    secret_obstacle: String,
    carrying: String,
    location: String,
    vault_path: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocationDeletePayload {
    id: String,
    slug: String,
    name: String,
    vault_path: String,
    kind_type: String,
    kind_custom: Option<String>,
    visual_description: String,
    history_background: String,
    exports: String,
    tone: String,
    authority: String,
    danger_level: String,
    current_tension: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FactionDeletePayload {
    id: String,
    slug: String,
    name: String,
    vault_path: String,
    kind_type: String,
    kind_custom: Option<String>,
    public_description: String,
    true_agenda: String,
    methods: String,
    leadership: String,
    headquarters: String,
    sphere_of_influence: String,
    resources_assets: String,
    allies: String,
    rivals_enemies: String,
    reputation: String,
    current_tension: String,
    goals_short_term: String,
    goals_long_term: String,
    symbol_description: String,
    created_at: String,
    updated_at: String,
}
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
) -> Result<CommandResponse, String> {
    let normalized_input = normalize_input_for_dispatch(&input);
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

    if let Some(response) =
        router::dispatch_desktop_command(&normalized_input, &parsed.normalized_tokens, state.clone())
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

fn normalize_input_for_dispatch(input: &str) -> String {
    normalize_command_input(input)
}

async fn ensure_location_exists(
    input: EnsureLocationInput,
    state: tauri::State<'_, AppState>,
) -> Result<EnsureLocationResult, String> {
    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let vault_path = loaded
        .effective
        .vault
        .path
        .clone()
        .ok_or_else(|| "vault.path is not configured".to_string())?;
    let vault = Vault::new(vault_path);
    state.vault_repo().ensure_structure(&vault)?;

    let raw_name = input.name.trim();
    if raw_name.is_empty() {
        return Err("location name cannot be empty".to_string());
    }
    if raw_name.eq_ignore_ascii_case(UNKNOWN_LOCATION) {
        return Ok(EnsureLocationResult {
            name: UNKNOWN_LOCATION.to_string(),
            slug: slugify(UNKNOWN_LOCATION),
            vault_path: String::new(),
            created_file: false,
            created_record: false,
        });
    }

    let database = state.database();
    let location_repo = state.location_repo();
    let document_repo = state.document_repo();
    let slug = slugify(raw_name);
    let existing = location_repo
        .find_by_slug(database.as_ref(), &slug)
        .await?;

    let mut created_file = false;
    let mut created_record = false;
    let now = now_timestamp();
    let id = existing
        .as_ref()
        .map(|row| row.id.clone())
        .unwrap_or_else(|| make_entity_id("loc"));
    let canonical_name = existing
        .as_ref()
        .map(|row| row.name.clone())
        .unwrap_or_else(|| raw_name.to_string());
    let created_at = existing
        .as_ref()
        .map(|row| row.created_at.clone())
        .unwrap_or_else(|| now.clone());

    let relative_path = if let Some(row) = existing.as_ref() {
        normalize_relative_path_for_storage(&row.vault_path)
    } else {
        unique_markdown_path_for_name(&vault, "locations", &canonical_name, None)?
    };
    let file_exists = vault
        .resolve_relative(&PathBuf::from(&relative_path))
        .map_err(|err| err.to_string())?
        .exists();

    if !file_exists {
        let default_exports = vec!["Unknown".to_string()];
        let content = render_location_markdown(&LocationFrontmatter {
            doc_type: "location".to_string(),
            id: id.clone(),
            slug: slug.clone(),
            name: canonical_name.clone(),
            kind_type: "other".to_string(),
            kind_custom: Some("Unknown".to_string()),
            visual_description: "Unknown".to_string(),
            history_background: "Unknown".to_string(),
            exports: default_exports.clone(),
            tone: "Unknown".to_string(),
            authority: "Unknown".to_string(),
            danger_level: "Unknown".to_string(),
            current_tension: "Unknown".to_string(),
            created_at: created_at.clone(),
            updated_at: now.clone(),
        })
        .map_err(|err| err.to_string())?;
        vault
            .write_relative(&PathBuf::from(&relative_path), &content)
            .map_err(|err| err.to_string())?;
        created_file = true;
    }

    if existing.is_none() {
        created_record = true;
    }

    let row = db::LocationRow {
        id,
        slug: slug.clone(),
        name: canonical_name.clone(),
        vault_path: relative_path,
        kind_type: existing
            .as_ref()
            .map(|row| row.kind_type.clone())
            .unwrap_or_else(|| "other".to_string()),
        kind_custom: existing
            .as_ref()
            .and_then(|row| row.kind_custom.clone())
            .or_else(|| Some("Unknown".to_string())),
        visual_description: existing
            .as_ref()
            .map(|row| row.visual_description.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        history_background: existing
            .as_ref()
            .map(|row| row.history_background.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        exports: existing
            .as_ref()
            .map(|row| row.exports.clone())
            .unwrap_or_else(|| "[\"Unknown\"]".to_string()),
        tone: existing
            .as_ref()
            .map(|row| row.tone.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        authority: existing
            .as_ref()
            .map(|row| row.authority.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        danger_level: existing
            .as_ref()
            .map(|row| row.danger_level.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        current_tension: existing
            .as_ref()
            .map(|row| row.current_tension.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        created_at,
        updated_at: now.clone(),
    };

    location_repo
        .upsert(database.as_ref(), &row)
        .await?;
    document_repo
        .upsert_index(
            database.as_ref(),
            "location",
            &row.slug,
            Some(&row.name),
            &row.vault_path,
            &row.created_at,
            &row.updated_at,
        )
        .await?;

    Ok(EnsureLocationResult {
        name: canonical_name,
        slug,
        vault_path: row.vault_path,
        created_file,
        created_record,
    })
}

async fn resolve_entity(input: String, state: &AppState) -> Result<Option<EntityDetails>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let database = state.database();
    let npc_repo = state.npc_repo();
    let location_repo = state.location_repo();
    let faction_repo = state.faction_repo();

    if let Some(npc) = npc_repo
        .find_by_name_or_slug(database.as_ref(), trimmed)
        .await?
    {
        return Ok(Some(EntityDetails {
            id: npc.id,
            entity_type: EntityType::Npc,
            name: npc.name,
            slug: npc.slug,
            race: Some(npc.race),
            occupation: Some(npc.occupation),
            sex: Some(npc.sex),
            age: Some(npc.age),
            height: Some(npc.height),
            weight_lbs: Some(npc.weight_lbs),
            background: Some(npc.background),
            want_need: Some(npc.want_need),
            secret_obstacle: Some(npc.secret_obstacle),
            carrying: Some(carrying_from_db_text(&npc.carrying)),
            location: Some(npc.location),
            vault_path: normalize_relative_path_for_storage(&npc.vault_path),
            kind_type: None,
            kind_custom: None,
            visual_description: None,
            history_background: None,
            exports: None,
            tone: None,
            authority: None,
            danger_level: None,
            current_tension: None,
            public_description: None,
            true_agenda: None,
            methods: None,
            leadership: None,
            headquarters: None,
            sphere_of_influence: None,
            resources_assets: None,
            allies: None,
            rivals_enemies: None,
            reputation: None,
            goals_short_term: None,
            goals_long_term: None,
            symbol_description: None,
            created_at: Some(npc.created_at),
        }));
    }

    if let Some(location) = location_repo
        .find_by_name_or_slug(database.as_ref(), trimmed)
        .await?
    {
        return Ok(Some(EntityDetails {
            id: location.id,
            entity_type: EntityType::Location,
            name: location.name,
            slug: location.slug,
            race: None,
            occupation: None,
            sex: None,
            age: None,
            height: None,
            weight_lbs: None,
            background: None,
            want_need: None,
            secret_obstacle: None,
            carrying: None,
            location: None,
            vault_path: normalize_relative_path_for_storage(&location.vault_path),
            kind_type: Some(location.kind_type),
            kind_custom: location.kind_custom,
            visual_description: Some(location.visual_description),
            history_background: Some(location.history_background),
            exports: Some(exports_from_db_text(&location.exports)),
            tone: Some(location.tone),
            authority: Some(location.authority),
            danger_level: Some(location.danger_level),
            current_tension: Some(location.current_tension),
            public_description: None,
            true_agenda: None,
            methods: None,
            leadership: None,
            headquarters: None,
            sphere_of_influence: None,
            resources_assets: None,
            allies: None,
            rivals_enemies: None,
            reputation: None,
            goals_short_term: None,
            goals_long_term: None,
            symbol_description: None,
            created_at: Some(location.created_at),
        }));
    }

    if let Some(faction) = faction_repo
        .find_by_name_or_slug(database.as_ref(), trimmed)
        .await?
    {
        return Ok(Some(EntityDetails {
            id: faction.id,
            entity_type: EntityType::Faction,
            name: faction.name,
            slug: faction.slug,
            race: None,
            occupation: None,
            sex: None,
            age: None,
            height: None,
            weight_lbs: None,
            background: None,
            want_need: None,
            secret_obstacle: None,
            carrying: None,
            location: None,
            vault_path: normalize_relative_path_for_storage(&faction.vault_path),
            kind_type: Some(faction.kind_type),
            kind_custom: faction.kind_custom,
            visual_description: None,
            history_background: None,
            exports: None,
            tone: None,
            authority: None,
            danger_level: None,
            current_tension: Some(faction.current_tension),
            public_description: Some(faction.public_description),
            true_agenda: Some(faction.true_agenda),
            methods: Some(faction.methods),
            leadership: Some(faction.leadership),
            headquarters: Some(faction.headquarters),
            sphere_of_influence: Some(faction.sphere_of_influence),
            resources_assets: Some(faction.resources_assets),
            allies: Some(faction_list_from_db_text(&faction.allies)),
            rivals_enemies: Some(faction_list_from_db_text(&faction.rivals_enemies)),
            reputation: Some(faction.reputation),
            goals_short_term: Some(faction_list_from_db_text(&faction.goals_short_term)),
            goals_long_term: Some(faction_list_from_db_text(&faction.goals_long_term)),
            symbol_description: Some(faction.symbol_description),
            created_at: Some(faction.created_at),
        }));
    }

    Ok(None)
}

async fn soft_delete_entity(
    input: SoftDeleteEntityInput,
    state: tauri::State<'_, AppState>,
) -> Result<SoftDeleteEntityResult, String> {
    let target = input.target.trim();
    if target.is_empty() {
        return Err("usage: delete <npc-or-location-name>".to_string());
    }

    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let vault_path = loaded
        .effective
        .vault
        .path
        .clone()
        .ok_or_else(|| "vault.path is not configured".to_string())?;
    let vault = Vault::new(vault_path);
    state.vault_repo().ensure_structure(&vault)?;

    let database = state.database();
    let npc_repo = state.npc_repo();
    let location_repo = state.location_repo();
    let faction_repo = state.faction_repo();
    let document_repo = state.document_repo();
    let soft_delete_repo = state.soft_delete_repo();
    let now = now_timestamp();

    if let Some(npc) = npc_repo
        .find_by_name_or_slug(database.as_ref(), target)
        .await?
    {
        let normalized_vault_path = normalize_relative_path_for_storage(&npc.vault_path);
        let trash_path = unique_trash_path(&vault, "npcs", &npc.slug, &now)?;
        move_vault_file(&vault, &normalized_vault_path, &trash_path)?;

        npc_repo
            .delete_by_id(database.as_ref(), &npc.id)
            .await?;
        document_repo
            .delete_by_vault_path(database.as_ref(), &npc.vault_path)
            .await?;

        let payload = NpcDeletePayload {
            id: npc.id.clone(),
            slug: npc.slug.clone(),
            name: npc.name.clone(),
            race: npc.race,
            occupation: npc.occupation,
            sex: npc.sex,
            age: npc.age,
            height: npc.height,
            weight_lbs: npc.weight_lbs,
            background: npc.background,
            want_need: npc.want_need,
            secret_obstacle: npc.secret_obstacle,
            carrying: npc.carrying,
            location: npc.location,
            vault_path: normalized_vault_path.clone(),
            created_at: npc.created_at,
            updated_at: npc.updated_at,
        };

        let payload_json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
        let soft_delete_row = db::SoftDeleteRow {
            id: 0,
            entity_type: "npc".to_string(),
            entity_id: npc.id.clone(),
            name: npc.name.clone(),
            slug: npc.slug.clone(),
            original_vault_path: normalized_vault_path,
            trash_vault_path: trash_path.clone(),
            payload_json,
            created_at: now,
            undone_at: None,
        };
        soft_delete_repo
            .insert(database.as_ref(), &soft_delete_row)
            .await?;

        return Ok(SoftDeleteEntityResult {
            entity_type: EntityType::Npc,
            id: npc.id,
            name: npc.name,
            slug: npc.slug,
            trash_vault_path: trash_path,
        });
    }

    if let Some(location) = location_repo
        .find_by_name_or_slug(database.as_ref(), target)
        .await?
    {
        let normalized_vault_path = normalize_relative_path_for_storage(&location.vault_path);
        let trash_path = unique_trash_path(&vault, "locations", &location.slug, &now)?;
        move_vault_file(&vault, &normalized_vault_path, &trash_path)?;

        location_repo
            .delete_by_id(database.as_ref(), &location.id)
            .await?;
        document_repo
            .delete_by_vault_path(database.as_ref(), &location.vault_path)
            .await?;

        let payload = LocationDeletePayload {
            id: location.id.clone(),
            slug: location.slug.clone(),
            name: location.name.clone(),
            vault_path: normalized_vault_path.clone(),
            kind_type: location.kind_type,
            kind_custom: location.kind_custom,
            visual_description: location.visual_description,
            history_background: location.history_background,
            exports: location.exports,
            tone: location.tone,
            authority: location.authority,
            danger_level: location.danger_level,
            current_tension: location.current_tension,
            created_at: location.created_at,
            updated_at: location.updated_at,
        };

        let payload_json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
        let soft_delete_row = db::SoftDeleteRow {
            id: 0,
            entity_type: "location".to_string(),
            entity_id: location.id.clone(),
            name: location.name.clone(),
            slug: location.slug.clone(),
            original_vault_path: normalized_vault_path,
            trash_vault_path: trash_path.clone(),
            payload_json,
            created_at: now,
            undone_at: None,
        };
        soft_delete_repo
            .insert(database.as_ref(), &soft_delete_row)
            .await?;

        return Ok(SoftDeleteEntityResult {
            entity_type: EntityType::Location,
            id: location.id,
            name: location.name,
            slug: location.slug,
            trash_vault_path: trash_path,
        });
    }

    if let Some(faction) = faction_repo
        .find_by_name_or_slug(database.as_ref(), target)
        .await?
    {
        let normalized_vault_path = normalize_relative_path_for_storage(&faction.vault_path);
        let trash_path = unique_trash_path(&vault, "factions", &faction.slug, &now)?;
        move_vault_file(&vault, &normalized_vault_path, &trash_path)?;

        faction_repo
            .delete_by_id(database.as_ref(), &faction.id)
            .await?;
        document_repo
            .delete_by_vault_path(database.as_ref(), &faction.vault_path)
            .await?;

        let payload = FactionDeletePayload {
            id: faction.id.clone(),
            slug: faction.slug.clone(),
            name: faction.name.clone(),
            vault_path: normalized_vault_path.clone(),
            kind_type: faction.kind_type,
            kind_custom: faction.kind_custom,
            public_description: faction.public_description,
            true_agenda: faction.true_agenda,
            methods: faction.methods,
            leadership: faction.leadership,
            headquarters: faction.headquarters,
            sphere_of_influence: faction.sphere_of_influence,
            resources_assets: faction.resources_assets,
            allies: faction.allies,
            rivals_enemies: faction.rivals_enemies,
            reputation: faction.reputation,
            current_tension: faction.current_tension,
            goals_short_term: faction.goals_short_term,
            goals_long_term: faction.goals_long_term,
            symbol_description: faction.symbol_description,
            created_at: faction.created_at,
            updated_at: faction.updated_at,
        };

        let payload_json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
        let soft_delete_row = db::SoftDeleteRow {
            id: 0,
            entity_type: "faction".to_string(),
            entity_id: faction.id.clone(),
            name: faction.name.clone(),
            slug: faction.slug.clone(),
            original_vault_path: normalized_vault_path,
            trash_vault_path: trash_path.clone(),
            payload_json,
            created_at: now,
            undone_at: None,
        };
        soft_delete_repo
            .insert(database.as_ref(), &soft_delete_row)
            .await?;

        return Ok(SoftDeleteEntityResult {
            entity_type: EntityType::Faction,
            id: faction.id,
            name: faction.name,
            slug: faction.slug,
            trash_vault_path: trash_path,
        });
    }

    Err(format!("no npc, location, or faction found for: {target}"))
}

async fn undo_last_soft_delete(state: tauri::State<'_, AppState>) -> Result<UndoSoftDeleteResult, String> {
    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let vault_path = loaded
        .effective
        .vault
        .path
        .clone()
        .ok_or_else(|| "vault.path is not configured".to_string())?;
    let vault = Vault::new(vault_path);
    state.vault_repo().ensure_structure(&vault)?;

    let database = state.database();
    let npc_repo = state.npc_repo();
    let location_repo = state.location_repo();
    let faction_repo = state.faction_repo();
    let document_repo = state.document_repo();
    let soft_delete_repo = state.soft_delete_repo();

    let Some(soft_delete) = soft_delete_repo
        .latest_pending(database.as_ref())
        .await?
    else {
        return Err("nothing to undo".to_string());
    };

    let now = now_timestamp();

    if soft_delete.entity_type == "npc" {
        let payload: NpcDeletePayload =
            serde_json::from_str(&soft_delete.payload_json).map_err(|err| err.to_string())?;

        let mut restored_slug = payload.slug;
        let mut restored_vault_path = normalize_relative_path_for_storage(&payload.vault_path);
        let trash_vault_path = normalize_relative_path_for_storage(&soft_delete.trash_vault_path);
        let preferred_full = vault
            .resolve_relative(&PathBuf::from(&restored_vault_path))
            .map_err(|err| err.to_string())?;
        if preferred_full.exists() {
            restored_slug = unique_slug_for_dir(vault.root(), "npcs", &restored_slug);
            restored_vault_path = unique_markdown_path_for_name(&vault, "npcs", &payload.name, None)?;
        }

        move_vault_file(&vault, &trash_vault_path, &restored_vault_path)?;

        let npc_row = db::NpcRow {
            id: payload.id.clone(),
            slug: restored_slug.clone(),
            name: payload.name.clone(),
            race: payload.race,
            occupation: payload.occupation,
            sex: payload.sex,
            age: payload.age,
            height: payload.height,
            weight_lbs: payload.weight_lbs,
            background: payload.background,
            want_need: payload.want_need,
            secret_obstacle: payload.secret_obstacle,
            carrying: payload.carrying,
            location: payload.location,
            vault_path: restored_vault_path.clone(),
            created_at: payload.created_at,
            updated_at: now.clone(),
        };

        npc_repo
            .upsert(database.as_ref(), &npc_row)
            .await?;
        document_repo
            .upsert_index(
                database.as_ref(),
                "npc",
                &npc_row.slug,
                Some(&npc_row.name),
                &npc_row.vault_path,
                &npc_row.created_at,
                &npc_row.updated_at,
            )
            .await?;

        soft_delete_repo
            .mark_undone(database.as_ref(), soft_delete.id, &now)
            .await?;

        return Ok(UndoSoftDeleteResult {
            entity_type: EntityType::Npc,
            id: payload.id,
            name: payload.name,
            slug: restored_slug,
            vault_path: restored_vault_path,
        });
    }

    if soft_delete.entity_type == "location" {
        let payload: LocationDeletePayload =
            serde_json::from_str(&soft_delete.payload_json).map_err(|err| err.to_string())?;

        let mut restored_slug = payload.slug;
        let mut restored_vault_path = normalize_relative_path_for_storage(&payload.vault_path);
        let trash_vault_path = normalize_relative_path_for_storage(&soft_delete.trash_vault_path);
        let preferred_full = vault
            .resolve_relative(&PathBuf::from(&restored_vault_path))
            .map_err(|err| err.to_string())?;
        if preferred_full.exists() {
            restored_slug = unique_slug_for_dir(vault.root(), "locations", &restored_slug);
            restored_vault_path =
                unique_markdown_path_for_name(&vault, "locations", &payload.name, None)?;
        }

        move_vault_file(&vault, &trash_vault_path, &restored_vault_path)?;

        let location_row = db::LocationRow {
            id: payload.id.clone(),
            slug: restored_slug.clone(),
            name: payload.name.clone(),
            vault_path: restored_vault_path.clone(),
            kind_type: payload.kind_type,
            kind_custom: payload.kind_custom,
            visual_description: payload.visual_description,
            history_background: payload.history_background,
            exports: payload.exports,
            tone: payload.tone,
            authority: payload.authority,
            danger_level: payload.danger_level,
            current_tension: payload.current_tension,
            created_at: payload.created_at,
            updated_at: now.clone(),
        };

        location_repo
            .upsert(database.as_ref(), &location_row)
            .await?;
        document_repo
            .upsert_index(
                database.as_ref(),
                "location",
                &location_row.slug,
                Some(&location_row.name),
                &location_row.vault_path,
                &location_row.created_at,
                &location_row.updated_at,
            )
            .await?;

        soft_delete_repo
            .mark_undone(database.as_ref(), soft_delete.id, &now)
            .await?;

        return Ok(UndoSoftDeleteResult {
            entity_type: EntityType::Location,
            id: payload.id,
            name: payload.name,
            slug: restored_slug,
            vault_path: restored_vault_path,
        });
    }

    if soft_delete.entity_type == "faction" {
        let payload: FactionDeletePayload =
            serde_json::from_str(&soft_delete.payload_json).map_err(|err| err.to_string())?;

        let mut restored_slug = payload.slug;
        let mut restored_vault_path = normalize_relative_path_for_storage(&payload.vault_path);
        let trash_vault_path = normalize_relative_path_for_storage(&soft_delete.trash_vault_path);
        let preferred_full = vault
            .resolve_relative(&PathBuf::from(&restored_vault_path))
            .map_err(|err| err.to_string())?;
        if preferred_full.exists() {
            restored_slug = unique_slug_for_dir(vault.root(), "factions", &restored_slug);
            restored_vault_path =
                unique_markdown_path_for_name(&vault, "factions", &payload.name, None)?;
        }

        move_vault_file(&vault, &trash_vault_path, &restored_vault_path)?;

        let faction_row = db::FactionRow {
            id: payload.id.clone(),
            slug: restored_slug.clone(),
            name: payload.name.clone(),
            vault_path: restored_vault_path.clone(),
            kind_type: payload.kind_type,
            kind_custom: payload.kind_custom,
            public_description: payload.public_description,
            true_agenda: payload.true_agenda,
            methods: payload.methods,
            leadership: payload.leadership,
            headquarters: payload.headquarters,
            sphere_of_influence: payload.sphere_of_influence,
            resources_assets: payload.resources_assets,
            allies: payload.allies,
            rivals_enemies: payload.rivals_enemies,
            reputation: payload.reputation,
            current_tension: payload.current_tension,
            goals_short_term: payload.goals_short_term,
            goals_long_term: payload.goals_long_term,
            symbol_description: payload.symbol_description,
            created_at: payload.created_at,
            updated_at: now.clone(),
        };

        faction_repo
            .upsert(database.as_ref(), &faction_row)
            .await?;
        document_repo
            .upsert_index(
                database.as_ref(),
                "faction",
                &faction_row.slug,
                Some(&faction_row.name),
                &faction_row.vault_path,
                &faction_row.created_at,
                &faction_row.updated_at,
            )
            .await?;

        soft_delete_repo
            .mark_undone(database.as_ref(), soft_delete.id, &now)
            .await?;

        return Ok(UndoSoftDeleteResult {
            entity_type: EntityType::Faction,
            id: payload.id,
            name: payload.name,
            slug: restored_slug,
            vault_path: restored_vault_path,
        });
    }

    Err(format!(
        "unsupported soft delete entity type: {}",
        soft_delete.entity_type
    ))
}

#[tauri::command]
fn get_command_manifest() -> CommandManifest {
    dnd_core::command_manifest::command_manifest()
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::normalize_input_for_dispatch;
    use crate::services::ai_generation::LocationSeed;
    use crate::utils::{normalize_location_seed, validate_location_details};

    #[test]
    fn dispatch_preserves_windows_backslashes() {
        let input = r"set vault C:\Users\andrewk9\Documents\DND";
        assert_eq!(normalize_input_for_dispatch(input), input);
    }

    #[test]
    fn dispatch_only_unwraps_markdown_backticks() {
        let input = "  `set vault C:\\Users\\andrewk9\\Documents\\DND`  ";
        assert_eq!(
            normalize_input_for_dispatch(input),
            r"set vault C:\Users\andrewk9\Documents\DND"
        );
    }

    #[test]
    fn location_seed_requires_custom_kind_for_other() {
        let seed = LocationSeed {
            name: "Gloomreach".to_string(),
            kind_type: "other".to_string(),
            kind_custom: None,
            visual_description: "Moss-slick walls drip in torchlight.".to_string(),
            history_background: "Built by exiles. Later seized by smugglers.".to_string(),
            exports: vec!["amber resin".to_string()],
            tone: "wet tense".to_string(),
            authority: "Smuggler council".to_string(),
            danger_level: "risky".to_string(),
            current_tension: "A rival gang stalks the tunnels.".to_string(),
        };

        let err = normalize_location_seed(seed).expect_err("expected missing kind_custom error");
        assert!(err.contains("kind_custom"));
    }

    #[test]
    fn location_seed_validation_accepts_unknown_backcompat_values() {
        let seed = LocationSeed {
            name: "Unknown Hold".to_string(),
            kind_type: "other".to_string(),
            kind_custom: Some("Unknown".to_string()),
            visual_description: "Unknown".to_string(),
            history_background: "Unknown".to_string(),
            exports: vec!["Unknown".to_string()],
            tone: "Unknown".to_string(),
            authority: "Unknown".to_string(),
            danger_level: "Unknown".to_string(),
            current_tension: "Unknown".to_string(),
        };

        validate_location_details(&seed).expect("expected Unknown defaults to pass validation");
    }

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
    let document_repo: Arc<dyn DocumentRepository> = Arc::new(ProdDocumentRepository);
    let generation_repo: Arc<dyn GenerationRepository> = Arc::new(ProdGenerationRepository);
    let soft_delete_repo: Arc<dyn SoftDeleteRepository> = Arc::new(ProdSoftDeleteRepository);

    let command_service = dnd_core::service::CommandService::new(workspace_root.clone());

    let app_state = AppState {
        workspace_root,
        command_service: Mutex::new(command_service),
        editor_session: Mutex::new(EditorSession::default()),
        database: database.clone(),
        vault_repo: vault_repo.clone(),
        npc_repo: npc_repo.clone(),
        location_repo: location_repo.clone(),
        faction_repo: faction_repo.clone(),
        document_repo: document_repo.clone(),
        generation_repo: generation_repo.clone(),
        soft_delete_repo: soft_delete_repo.clone(),
    };

    let vault_sync_service = VaultSyncService;
    if let Err(err) = tauri::async_runtime::block_on(vault_sync_service.sync_from_vault(&app_state)) {
        eprintln!("startup vault sync skipped: {err}");
    }

    tauri::Builder::default()
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
