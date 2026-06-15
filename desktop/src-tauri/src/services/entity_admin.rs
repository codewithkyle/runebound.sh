use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::repositories::db;
use crate::services::vault_sync::{move_vault_file, unique_markdown_path_for_name, unique_trash_path};
use crate::utils::normalize_relative_path_for_storage;
use dnd_core::config::{load_effective, validate_for_runtime};
use dnd_core::npc::{
    LocationFrontmatter, UNKNOWN_LOCATION, make_entity_id, now_timestamp, render_location_markdown,
    slugify, unique_slug_for_dir,
};
use dnd_core::serialization::{
    carrying_from_db_text, exports_from_db_text, faction_list_from_db_text,
};
use dnd_core::vault::Vault;

pub struct EntityAdminService;

impl EntityAdminService {
    pub async fn ensure_location_exists(
        &self,
        input: EnsureLocationInput,
        state: &AppState,
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

    pub async fn resolve_entity(
        &self,
        input: String,
        state: &AppState,
    ) -> Result<Option<EntityDetails>, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        let database = state.database();
        let npc_repo = state.npc_repo();
        let location_repo = state.location_repo();
        let faction_repo = state.faction_repo();
        let item_repo = state.item_repo();

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
                category: None,
                rarity: None,
                attunement: None,
                materials: None,
                appearance: None,
                abilities: None,
                drawbacks: None,
                history: None,
                value: None,
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
                category: None,
                rarity: None,
                attunement: None,
                materials: None,
                appearance: None,
                abilities: None,
                drawbacks: None,
                history: None,
                value: None,
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
                category: None,
                rarity: None,
                attunement: None,
                materials: None,
                appearance: None,
                abilities: None,
                drawbacks: None,
                history: None,
                value: None,
                created_at: Some(faction.created_at),
            }));
        }

        if let Some(item) = item_repo
            .find_by_name_or_slug(database.as_ref(), trimmed)
            .await?
        {
            return Ok(Some(EntityDetails {
                id: item.id,
                entity_type: EntityType::Item,
                name: item.name,
                slug: item.slug,
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
                location: Some(item.location.clone()),
                vault_path: normalize_relative_path_for_storage(&item.vault_path),
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
                category: Some(item.category),
                rarity: Some(item.rarity),
                attunement: Some(item.attunement),
                materials: Some(faction_list_from_db_text(&item.materials)),
                appearance: Some(item.appearance),
                abilities: Some(item.abilities),
                drawbacks: Some(item.drawbacks),
                history: Some(item.history),
                value: Some(item.value),
                created_at: Some(item.created_at),
            }));
        }

        Ok(None)
    }

    pub async fn soft_delete_entity(
        &self,
        input: SoftDeleteEntityInput,
        state: &AppState,
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
        let item_repo = state.item_repo();
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
                created_at: now.clone(),
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
                created_at: now.clone(),
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
                created_at: now.clone(),
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

        if let Some(item) = item_repo
            .find_by_name_or_slug(database.as_ref(), target)
            .await?
        {
            let normalized_vault_path = normalize_relative_path_for_storage(&item.vault_path);
            let trash_path = unique_trash_path(&vault, "items", &item.slug, &now)?;
            move_vault_file(&vault, &normalized_vault_path, &trash_path)?;

            item_repo
                .delete_by_id(database.as_ref(), &item.id)
                .await?;
            document_repo
                .delete_by_vault_path(database.as_ref(), &item.vault_path)
                .await?;

            let payload = ItemDeletePayload {
                id: item.id.clone(),
                slug: item.slug.clone(),
                name: item.name.clone(),
                vault_path: normalized_vault_path.clone(),
                category: item.category,
                rarity: item.rarity,
                attunement: item.attunement,
                materials: item.materials,
                appearance: item.appearance,
                abilities: item.abilities,
                drawbacks: item.drawbacks,
                history: item.history,
                value: item.value,
                location: item.location,
                created_at: item.created_at,
                updated_at: item.updated_at,
            };

            let payload_json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
            let soft_delete_row = db::SoftDeleteRow {
                id: 0,
                entity_type: "item".to_string(),
                entity_id: item.id.clone(),
                name: item.name.clone(),
                slug: item.slug.clone(),
                original_vault_path: normalized_vault_path,
                trash_vault_path: trash_path.clone(),
                payload_json,
                created_at: now.clone(),
                undone_at: None,
            };
            soft_delete_repo
                .insert(database.as_ref(), &soft_delete_row)
                .await?;

            return Ok(SoftDeleteEntityResult {
                entity_type: EntityType::Item,
                id: item.id,
                name: item.name,
                slug: item.slug,
                trash_vault_path: trash_path,
            });
        }

        Err(format!("no npc, location, faction, or item found for: {target}"))
    }

    pub async fn undo_last_soft_delete(
        &self,
        state: &AppState,
    ) -> Result<UndoSoftDeleteResult, String> {
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
        let item_repo = state.item_repo();
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
                restored_vault_path =
                    unique_markdown_path_for_name(&vault, "npcs", &payload.name, None)?;
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

        if soft_delete.entity_type == "item" {
            let payload: ItemDeletePayload =
                serde_json::from_str(&soft_delete.payload_json).map_err(|err| err.to_string())?;

            let mut restored_slug = payload.slug.clone();
            let mut restored_vault_path = normalize_relative_path_for_storage(&payload.vault_path);
            let trash_vault_path = normalize_relative_path_for_storage(&soft_delete.trash_vault_path);
            let preferred_full = vault
                .resolve_relative(&PathBuf::from(&restored_vault_path))
                .map_err(|err| err.to_string())?;
            if preferred_full.exists() {
                restored_slug = unique_slug_for_dir(vault.root(), "items", &restored_slug);
                restored_vault_path = unique_markdown_path_for_name(&vault, "items", &payload.name, None)?;
            }

            move_vault_file(&vault, &trash_vault_path, &restored_vault_path)?;

            let item_row = db::ItemRow {
                id: payload.id.clone(),
                slug: restored_slug.clone(),
                name: payload.name.clone(),
                vault_path: restored_vault_path.clone(),
                category: payload.category,
                rarity: payload.rarity,
                attunement: payload.attunement,
                materials: payload.materials,
                appearance: payload.appearance,
                abilities: payload.abilities,
                drawbacks: payload.drawbacks,
                history: payload.history,
                value: payload.value,
                location: payload.location,
                created_at: payload.created_at,
                updated_at: now.clone(),
            };

            item_repo
                .upsert(database.as_ref(), &item_row)
                .await?;
            document_repo
                .upsert_index(
                    database.as_ref(),
                    "item",
                    &item_row.slug,
                    Some(&item_row.name),
                    &item_row.vault_path,
                    &item_row.created_at,
                    &item_row.updated_at,
                )
                .await?;

            soft_delete_repo
                .mark_undone(database.as_ref(), soft_delete.id, &now)
                .await?;

            return Ok(UndoSoftDeleteResult {
                entity_type: EntityType::Item,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct SoftDeleteEntityInput {
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SoftDeleteEntityResult {
    pub entity_type: EntityType,
    pub id: String,
    pub name: String,
    pub slug: String,
    pub trash_vault_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UndoSoftDeleteResult {
    pub entity_type: EntityType,
    pub id: String,
    pub name: String,
    pub slug: String,
    pub vault_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnsureLocationInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnsureLocationResult {
    pub name: String,
    pub slug: String,
    pub vault_path: String,
    pub created_file: bool,
    pub created_record: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Npc,
    Location,
    Faction,
    Item,
}

impl EntityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityType::Npc => "npc",
            EntityType::Location => "location",
            EntityType::Faction => "faction",
            EntityType::Item => "item",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityDetails {
    pub id: String,
    pub entity_type: EntityType,
    pub name: String,
    pub slug: String,
    pub race: Option<String>,
    pub occupation: Option<String>,
    pub sex: Option<String>,
    pub age: Option<String>,
    pub height: Option<String>,
    pub weight_lbs: Option<String>,
    pub background: Option<String>,
    pub want_need: Option<String>,
    pub secret_obstacle: Option<String>,
    pub carrying: Option<Vec<String>>,
    pub location: Option<String>,
    pub vault_path: String,
    pub kind_type: Option<String>,
    pub kind_custom: Option<String>,
    pub visual_description: Option<String>,
    pub history_background: Option<String>,
    pub exports: Option<Vec<String>>,
    pub tone: Option<String>,
    pub authority: Option<String>,
    pub danger_level: Option<String>,
    pub current_tension: Option<String>,
    pub public_description: Option<String>,
    pub true_agenda: Option<String>,
    pub methods: Option<String>,
    pub leadership: Option<String>,
    pub headquarters: Option<String>,
    pub sphere_of_influence: Option<String>,
    pub resources_assets: Option<String>,
    pub allies: Option<Vec<String>>,
    pub rivals_enemies: Option<Vec<String>>,
    pub reputation: Option<String>,
    pub goals_short_term: Option<Vec<String>>,
    pub goals_long_term: Option<Vec<String>>,
    pub symbol_description: Option<String>,
    pub category: Option<String>,
    pub rarity: Option<String>,
    pub attunement: Option<String>,
    pub materials: Option<Vec<String>>,
    pub appearance: Option<String>,
    pub abilities: Option<String>,
    pub drawbacks: Option<String>,
    pub history: Option<String>,
    pub value: Option<String>,
    pub created_at: Option<String>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ItemDeletePayload {
    id: String,
    slug: String,
    name: String,
    vault_path: String,
    category: String,
    rarity: String,
    attunement: String,
    materials: String,
    appearance: String,
    abilities: String,
    drawbacks: String,
    history: String,
    value: String,
    location: String,
    created_at: String,
    updated_at: String,
}
