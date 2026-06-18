use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::entities::{ALL_ENTITY_KINDS, EntityDetail, EntityKind};
use crate::repositories::db;
use crate::services::entity_persistence::unique_readable_vault_path;
use crate::services::publish::render_location_markdown;
use crate::services::vault_sync::{
    dungeon_row_from_frontmatter, event_row_from_frontmatter, faction_row_from_frontmatter,
    god_row_from_frontmatter, item_row_from_frontmatter, location_row_from_frontmatter,
    npc_row_from_frontmatter, unique_markdown_path_for_name,
};
use crate::utils::normalize_relative_path_for_storage;
use dnd_core::config::{load_effective, validate_for_runtime};
use dnd_core::entity_store::EntityStore;
use dnd_core::npc::{
    DungeonFrontmatter, EventFrontmatter, FactionFrontmatter, GodFrontmatter, ItemFrontmatter,
    LocationFrontmatter, NpcFrontmatter, UNKNOWN_LOCATION, make_entity_id, now_timestamp, slugify,
    unique_slug_for_dir_with_ext,
};
use dnd_core::vault::Vault;

/// Generate one entity's `soft_delete_<kind>` + `restore_<kind>` free functions
/// (P5.2d). Both wrap the shared `record_soft_delete` / `restore_collision` +
/// `commit_restore` helpers around the per-kind repo + TOML store.
///
/// Delete and undo operate ENTIRELY on local state — the TOML draft (the entity
/// store), the DB row, and the document index. They never read, move, or remove
/// anything in the user's Obsidian vault: deleting a saved-but-unpublished draft
/// must not depend on a vault `.md` that publish hasn't written yet, and we never
/// destroy a vault file the user owns. The serialized TOML frontmatter IS the
/// recovery payload (it preserves `published_at`, which the DB row drops), so
/// undo recreates the draft and re-derives the DB row from it. `soft_delete`
/// writes the recovery row before any destructive step (P1.3).
macro_rules! impl_entity_soft_delete {
    (
        kind: $kind:expr,
        dir: $dir:literal,
        repo: $repo:ident,
        frontmatter: $Frontmatter:ty,
        store_load: $store_load:ident,
        store_save: $store_save:ident,
        store_delete: $store_delete:ident,
        row_from_frontmatter: $row_from_frontmatter:ident,
        soft_delete_fn: $sd:ident,
        restore_fn: $rs:ident $(,)?
    ) => {
        async fn $sd(
            target: &str,
            state: &AppState,
            store: &EntityStore,
            now: &str,
        ) -> Result<Option<SoftDeleteEntityResult>, String> {
            let database = state.database();
            let Some(row) = state
                .$repo()
                .find_by_name_or_slug(database.as_ref(), target)
                .await?
            else {
                return Ok(None);
            };
            // The local TOML draft is the recovery payload — never the vault file.
            let frontmatter = store
                .$store_load(&row.slug)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| {
                    format!("cannot delete \"{}\": its draft file is missing", row.name)
                })?;
            let payload_json =
                serde_json::to_string(&frontmatter).map_err(|err| err.to_string())?;
            record_soft_delete(
                state,
                $kind,
                &row.id,
                &row.slug,
                &row.name,
                &row.vault_path,
                payload_json,
                now,
            )
            .await?;
            // Destructive steps run only after the recovery row is committed (P1.3).
            // We remove the local TOML draft + DB/index rows; the vault is untouched.
            store
                .$store_delete(&row.slug)
                .map_err(|err| err.to_string())?;
            state
                .$repo()
                .delete_by_id(database.as_ref(), &row.id)
                .await?;
            state
                .document_repo()
                .delete_by_vault_path(database.as_ref(), &row.vault_path)
                .await?;
            Ok(Some(SoftDeleteEntityResult {
                entity_type: $kind,
                id: row.id,
                name: row.name,
                slug: row.slug,
            }))
        }

        async fn $rs(
            soft_delete: &db::SoftDeleteRow,
            state: &AppState,
            store: &EntityStore,
            now: &str,
        ) -> Result<UndoSoftDeleteResult, String> {
            let database = state.database();
            let mut frontmatter: $Frontmatter =
                serde_json::from_str(&soft_delete.payload_json).map_err(|err| err.to_string())?;
            // Re-mint slug/vault path only if a new entity took them while this one
            // was deleted. Collisions are checked against the local store + document
            // index — never the vault.
            let (restored_slug, restored_vault_path) = restore_collision(
                state,
                store,
                $dir,
                &frontmatter.slug,
                &frontmatter.name,
                &frontmatter.vault_path,
            )
            .await?;
            frontmatter.slug = restored_slug.clone();
            frontmatter.vault_path = restored_vault_path.clone();
            frontmatter.updated_at = now.to_string();
            // Recreate the TOML draft, then re-derive the DB row + document index.
            store
                .$store_save(&frontmatter)
                .map_err(|err| err.to_string())?;
            let row = $row_from_frontmatter(&frontmatter)?;
            state.$repo().upsert(database.as_ref(), &row).await?;
            commit_restore(
                state,
                $kind,
                &row.slug,
                &row.name,
                &row.vault_path,
                &row.created_at,
                &row.updated_at,
                soft_delete.id,
                now,
            )
            .await?;
            Ok(UndoSoftDeleteResult {
                entity_type: $kind,
                id: row.id,
                name: row.name,
                slug: restored_slug,
                vault_path: restored_vault_path,
            })
        }
    };
}

pub struct EntityAdminService;

impl EntityAdminService {
    pub async fn ensure_location_exists(
        &self,
        input: EnsureLocationInput,
        state: &AppState,
    ) -> Result<EnsureLocationResult, String> {
        let loaded = load_effective().map_err(|err| err.to_string())?;
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
        let existing = location_repo.find_by_slug(database.as_ref(), &slug).await?;

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
                vault_path: relative_path.clone(),
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
                published_at: None,
            });
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

        location_repo.upsert(database.as_ref(), &row).await?;
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
    ) -> Result<Option<EntityDetail>, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        // Drive resolution through the domain registry: ask every kind, then
        // disambiguate. Collecting all matches (rather than returning the first)
        // is what lets a cross-kind name collision surface as an error instead of
        // silently shadowing the kinds later in the walk order (P5.7).
        let registry = state.domains();
        let mut matches: Vec<EntityDetail> = Vec::new();
        for kind in ALL_ENTITY_KINDS {
            if let Some(domain) = registry.domain(kind)
                && let Some(detail) = domain.resolve(trimmed, state).await?
            {
                matches.push(detail);
            }
        }

        match matches.len() {
            0 => Ok(None),
            1 => Ok(matches.pop()),
            _ => {
                let name = matches[0].name().to_string();
                let kinds = matches
                    .iter()
                    .map(|detail| detail.kind().display_name())
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(format!(
                    "\"{name}\" matches multiple saved entities ({kinds}). Names must be \
                     unique to load, show, or delete by name — rename one so the name no \
                     longer collides."
                ))
            }
        }
    }

    pub async fn soft_delete_entity(
        &self,
        input: SoftDeleteEntityInput,
        state: &AppState,
    ) -> Result<SoftDeleteEntityResult, String> {
        let target = input.target.trim();
        if target.is_empty() {
            return Err("usage: delete <entity-name>".to_string());
        }

        // Delete works purely on local state, so it needs no vault config \u2014 only the
        // entity store (TOML drafts) and the database.
        let store = EntityStore::new().map_err(|err| err.to_string())?;
        let now = now_timestamp();

        // Walk kinds in registry order, soft-deleting the first name/slug match
        // (first-match wins \u2014 a bare name colliding across kinds resolves to the
        // earliest kind). The per-kind snapshot + draft/DB/index removal is generated
        // by `impl_entity_soft_delete!`; the vault is never touched.
        for kind in ALL_ENTITY_KINDS {
            let found = match kind {
                EntityKind::Npc => soft_delete_npc(target, state, &store, &now).await?,
                EntityKind::Location => soft_delete_location(target, state, &store, &now).await?,
                EntityKind::Faction => soft_delete_faction(target, state, &store, &now).await?,
                EntityKind::Item => soft_delete_item(target, state, &store, &now).await?,
                EntityKind::Event => soft_delete_event(target, state, &store, &now).await?,
                EntityKind::God => soft_delete_god(target, state, &store, &now).await?,
                EntityKind::Dungeon => soft_delete_dungeon(target, state, &store, &now).await?,
            };
            if let Some(result) = found {
                return Ok(result);
            }
        }

        Err(format!(
            "nothing to delete: no saved draft found for \"{target}\"."
        ))
    }

    pub async fn undo_last_soft_delete(
        &self,
        state: &AppState,
    ) -> Result<UndoSoftDeleteResult, String> {
        let database = state.database();
        let soft_delete_repo = state.soft_delete_repo();

        let Some(soft_delete) = soft_delete_repo.latest_pending(database.as_ref()).await? else {
            return Err("nothing to undo".to_string());
        };

        if soft_delete.operation == "publish" {
            return self.undo_publish(state, &soft_delete).await;
        }

        // Undo of a delete is local-only too: recreate the TOML draft + DB/index
        // rows from the recovery payload. No vault config or file moves involved.
        let store = EntityStore::new().map_err(|err| err.to_string())?;
        let now = now_timestamp();
        match soft_delete.entity_type.as_str() {
            "npc" => restore_npc(&soft_delete, state, &store, &now).await,
            "location" => restore_location(&soft_delete, state, &store, &now).await,
            "faction" => restore_faction(&soft_delete, state, &store, &now).await,
            "item" => restore_item(&soft_delete, state, &store, &now).await,
            "event" => restore_event(&soft_delete, state, &store, &now).await,
            "god" => restore_god(&soft_delete, state, &store, &now).await,
            "dungeon" => restore_dungeon(&soft_delete, state, &store, &now).await,
            other => Err(format!("unsupported soft delete entity type: {other}")),
        }
    }

    /// Retires a just-published entity from the app: records a reversible `publish`
    /// recovery row, then removes the DB row + document index so it no longer
    /// appears in typeaheads and can't be edited/previewed. The canonical TOML
    /// (with `published_at` set) and the published vault `.md` are left untouched.
    pub async fn soft_delete_for_publish(
        &self,
        state: &AppState,
        entity_type: EntityKind,
        slug: &str,
    ) -> Result<(), String> {
        let database = state.database();
        let document_repo = state.document_repo();
        let soft_delete_repo = state.soft_delete_repo();
        let now = now_timestamp();

        // Look up the live row (without deleting yet) so the recovery record is
        // written before anything is destroyed.
        let Some((id, name, vault_path)) = (match entity_type {
            EntityKind::Npc => state
                .npc_repo()
                .find_by_name_or_slug(database.as_ref(), slug)
                .await?
                .map(|row| (row.id, row.name, row.vault_path)),
            EntityKind::Location => state
                .location_repo()
                .find_by_name_or_slug(database.as_ref(), slug)
                .await?
                .map(|row| (row.id, row.name, row.vault_path)),
            EntityKind::Faction => state
                .faction_repo()
                .find_by_name_or_slug(database.as_ref(), slug)
                .await?
                .map(|row| (row.id, row.name, row.vault_path)),
            EntityKind::Item => state
                .item_repo()
                .find_by_name_or_slug(database.as_ref(), slug)
                .await?
                .map(|row| (row.id, row.name, row.vault_path)),
            EntityKind::Event => state
                .event_repo()
                .find_by_name_or_slug(database.as_ref(), slug)
                .await?
                .map(|row| (row.id, row.name, row.vault_path)),
            EntityKind::God => state
                .god_repo()
                .find_by_name_or_slug(database.as_ref(), slug)
                .await?
                .map(|row| (row.id, row.name, row.vault_path)),
            EntityKind::Dungeon => state
                .dungeon_repo()
                .find_by_name_or_slug(database.as_ref(), slug)
                .await?
                .map(|row| (row.id, row.name, row.vault_path)),
        }) else {
            // Already gone from the DB (e.g. double publish) — nothing to retire.
            return Ok(());
        };

        let normalized = normalize_relative_path_for_storage(&vault_path);
        let payload = PublishPayload {
            id: id.clone(),
            slug: slug.to_string(),
            name: name.clone(),
            vault_path: normalized.clone(),
        };
        let payload_json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;

        let soft_delete_row = db::SoftDeleteRow {
            id: 0,
            entity_type: entity_type.as_str().to_string(),
            entity_id: id.clone(),
            name,
            slug: slug.to_string(),
            original_vault_path: normalized.clone(),
            trash_vault_path: String::new(),
            payload_json,
            created_at: now,
            undone_at: None,
            operation: "publish".to_string(),
        };
        soft_delete_repo
            .insert(database.as_ref(), &soft_delete_row)
            .await?;

        match entity_type {
            EntityKind::Npc => {
                state
                    .npc_repo()
                    .delete_by_id(database.as_ref(), &id)
                    .await?
            }
            EntityKind::Location => {
                state
                    .location_repo()
                    .delete_by_id(database.as_ref(), &id)
                    .await?
            }
            EntityKind::Faction => {
                state
                    .faction_repo()
                    .delete_by_id(database.as_ref(), &id)
                    .await?
            }
            EntityKind::Item => {
                state
                    .item_repo()
                    .delete_by_id(database.as_ref(), &id)
                    .await?
            }
            EntityKind::Event => {
                state
                    .event_repo()
                    .delete_by_id(database.as_ref(), &id)
                    .await?
            }
            EntityKind::God => {
                state
                    .god_repo()
                    .delete_by_id(database.as_ref(), &id)
                    .await?
            }
            EntityKind::Dungeon => {
                state
                    .dungeon_repo()
                    .delete_by_id(database.as_ref(), &id)
                    .await?
            }
        }
        document_repo
            .delete_by_vault_path(database.as_ref(), &normalized)
            .await?;

        Ok(())
    }

    /// Reverses a `publish`: restores the entity DB row + document index from the
    /// canonical store and clears `published_at`. The vault `.md` is left in place.
    async fn undo_publish(
        &self,
        state: &AppState,
        soft_delete: &db::SoftDeleteRow,
    ) -> Result<UndoSoftDeleteResult, String> {
        let database = state.database();
        let document_repo = state.document_repo();
        let soft_delete_repo = state.soft_delete_repo();
        let store = EntityStore::new().map_err(|err| err.to_string())?;
        let now = now_timestamp();
        let slug = soft_delete.slug.as_str();
        let missing = || {
            format!(
                "cannot undo publish: canonical {} record is missing",
                soft_delete.entity_type
            )
        };

        match soft_delete.entity_type.as_str() {
            "npc" => {
                let mut frontmatter = store
                    .load_npc(slug)
                    .map_err(|err| err.to_string())?
                    .ok_or_else(missing)?;
                frontmatter.published_at = None;
                store
                    .save_npc(&frontmatter)
                    .map_err(|err| err.to_string())?;
                let row = npc_row_from_frontmatter(&frontmatter)?;
                state.npc_repo().upsert(database.as_ref(), &row).await?;
                document_repo
                    .upsert_index(
                        database.as_ref(),
                        "npc",
                        &row.slug,
                        Some(&row.name),
                        &row.vault_path,
                        &row.created_at,
                        &row.updated_at,
                    )
                    .await?;
                soft_delete_repo
                    .mark_undone(database.as_ref(), soft_delete.id, &now)
                    .await?;
                Ok(UndoSoftDeleteResult {
                    entity_type: EntityKind::Npc,
                    id: frontmatter.id,
                    name: frontmatter.name,
                    slug: frontmatter.slug,
                    vault_path: frontmatter.vault_path,
                })
            }
            "location" => {
                let mut frontmatter = store
                    .load_location(slug)
                    .map_err(|err| err.to_string())?
                    .ok_or_else(missing)?;
                frontmatter.published_at = None;
                store
                    .save_location(&frontmatter)
                    .map_err(|err| err.to_string())?;
                let row = location_row_from_frontmatter(&frontmatter)?;
                state
                    .location_repo()
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
                soft_delete_repo
                    .mark_undone(database.as_ref(), soft_delete.id, &now)
                    .await?;
                Ok(UndoSoftDeleteResult {
                    entity_type: EntityKind::Location,
                    id: frontmatter.id,
                    name: frontmatter.name,
                    slug: frontmatter.slug,
                    vault_path: frontmatter.vault_path,
                })
            }
            "faction" => {
                let mut frontmatter = store
                    .load_faction(slug)
                    .map_err(|err| err.to_string())?
                    .ok_or_else(missing)?;
                frontmatter.published_at = None;
                store
                    .save_faction(&frontmatter)
                    .map_err(|err| err.to_string())?;
                let row = faction_row_from_frontmatter(&frontmatter)?;
                state.faction_repo().upsert(database.as_ref(), &row).await?;
                document_repo
                    .upsert_index(
                        database.as_ref(),
                        "faction",
                        &row.slug,
                        Some(&row.name),
                        &row.vault_path,
                        &row.created_at,
                        &row.updated_at,
                    )
                    .await?;
                soft_delete_repo
                    .mark_undone(database.as_ref(), soft_delete.id, &now)
                    .await?;
                Ok(UndoSoftDeleteResult {
                    entity_type: EntityKind::Faction,
                    id: frontmatter.id,
                    name: frontmatter.name,
                    slug: frontmatter.slug,
                    vault_path: frontmatter.vault_path,
                })
            }
            "item" => {
                let mut frontmatter = store
                    .load_item(slug)
                    .map_err(|err| err.to_string())?
                    .ok_or_else(missing)?;
                frontmatter.published_at = None;
                store
                    .save_item(&frontmatter)
                    .map_err(|err| err.to_string())?;
                let row = item_row_from_frontmatter(&frontmatter)?;
                state.item_repo().upsert(database.as_ref(), &row).await?;
                document_repo
                    .upsert_index(
                        database.as_ref(),
                        "item",
                        &row.slug,
                        Some(&row.name),
                        &row.vault_path,
                        &row.created_at,
                        &row.updated_at,
                    )
                    .await?;
                soft_delete_repo
                    .mark_undone(database.as_ref(), soft_delete.id, &now)
                    .await?;
                Ok(UndoSoftDeleteResult {
                    entity_type: EntityKind::Item,
                    id: frontmatter.id,
                    name: frontmatter.name,
                    slug: frontmatter.slug,
                    vault_path: frontmatter.vault_path,
                })
            }
            "event" => {
                let mut frontmatter = store
                    .load_event(slug)
                    .map_err(|err| err.to_string())?
                    .ok_or_else(missing)?;
                frontmatter.published_at = None;
                store
                    .save_event(&frontmatter)
                    .map_err(|err| err.to_string())?;
                let row = event_row_from_frontmatter(&frontmatter)?;
                state.event_repo().upsert(database.as_ref(), &row).await?;
                document_repo
                    .upsert_index(
                        database.as_ref(),
                        "event",
                        &row.slug,
                        Some(&row.name),
                        &row.vault_path,
                        &row.created_at,
                        &row.updated_at,
                    )
                    .await?;
                soft_delete_repo
                    .mark_undone(database.as_ref(), soft_delete.id, &now)
                    .await?;
                Ok(UndoSoftDeleteResult {
                    entity_type: EntityKind::Event,
                    id: frontmatter.id,
                    name: frontmatter.name,
                    slug: frontmatter.slug,
                    vault_path: frontmatter.vault_path,
                })
            }
            "god" => {
                let mut frontmatter = store
                    .load_god(slug)
                    .map_err(|err| err.to_string())?
                    .ok_or_else(missing)?;
                frontmatter.published_at = None;
                store
                    .save_god(&frontmatter)
                    .map_err(|err| err.to_string())?;
                let row = god_row_from_frontmatter(&frontmatter)?;
                state.god_repo().upsert(database.as_ref(), &row).await?;
                document_repo
                    .upsert_index(
                        database.as_ref(),
                        "god",
                        &row.slug,
                        Some(&row.name),
                        &row.vault_path,
                        &row.created_at,
                        &row.updated_at,
                    )
                    .await?;
                soft_delete_repo
                    .mark_undone(database.as_ref(), soft_delete.id, &now)
                    .await?;
                Ok(UndoSoftDeleteResult {
                    entity_type: EntityKind::God,
                    id: frontmatter.id,
                    name: frontmatter.name,
                    slug: frontmatter.slug,
                    vault_path: frontmatter.vault_path,
                })
            }
            "dungeon" => {
                let mut frontmatter = store
                    .load_dungeon(slug)
                    .map_err(|err| err.to_string())?
                    .ok_or_else(missing)?;
                frontmatter.published_at = None;
                store
                    .save_dungeon(&frontmatter)
                    .map_err(|err| err.to_string())?;
                let row = dungeon_row_from_frontmatter(&frontmatter)?;
                state.dungeon_repo().upsert(database.as_ref(), &row).await?;
                document_repo
                    .upsert_index(
                        database.as_ref(),
                        "dungeon",
                        &row.slug,
                        Some(&row.name),
                        &row.vault_path,
                        &row.created_at,
                        &row.updated_at,
                    )
                    .await?;
                soft_delete_repo
                    .mark_undone(database.as_ref(), soft_delete.id, &now)
                    .await?;
                Ok(UndoSoftDeleteResult {
                    entity_type: EntityKind::Dungeon,
                    id: frontmatter.id,
                    name: frontmatter.name,
                    slug: frontmatter.slug,
                    vault_path: frontmatter.vault_path,
                })
            }
            other => Err(format!(
                "cannot undo publish for unknown entity type: {other}"
            )),
        }
    }
}

impl_entity_soft_delete! {
    kind: EntityKind::Npc,
    dir: "npcs",
    repo: npc_repo,
    frontmatter: NpcFrontmatter,
    store_load: load_npc,
    store_save: save_npc,
    store_delete: delete_npc,
    row_from_frontmatter: npc_row_from_frontmatter,
    soft_delete_fn: soft_delete_npc,
    restore_fn: restore_npc,
}
impl_entity_soft_delete! {
    kind: EntityKind::Location,
    dir: "locations",
    repo: location_repo,
    frontmatter: LocationFrontmatter,
    store_load: load_location,
    store_save: save_location,
    store_delete: delete_location,
    row_from_frontmatter: location_row_from_frontmatter,
    soft_delete_fn: soft_delete_location,
    restore_fn: restore_location,
}
impl_entity_soft_delete! {
    kind: EntityKind::Faction,
    dir: "factions",
    repo: faction_repo,
    frontmatter: FactionFrontmatter,
    store_load: load_faction,
    store_save: save_faction,
    store_delete: delete_faction,
    row_from_frontmatter: faction_row_from_frontmatter,
    soft_delete_fn: soft_delete_faction,
    restore_fn: restore_faction,
}
impl_entity_soft_delete! {
    kind: EntityKind::Item,
    dir: "items",
    repo: item_repo,
    frontmatter: ItemFrontmatter,
    store_load: load_item,
    store_save: save_item,
    store_delete: delete_item,
    row_from_frontmatter: item_row_from_frontmatter,
    soft_delete_fn: soft_delete_item,
    restore_fn: restore_item,
}
impl_entity_soft_delete! {
    kind: EntityKind::Event,
    dir: "events",
    repo: event_repo,
    frontmatter: EventFrontmatter,
    store_load: load_event,
    store_save: save_event,
    store_delete: delete_event,
    row_from_frontmatter: event_row_from_frontmatter,
    soft_delete_fn: soft_delete_event,
    restore_fn: restore_event,
}
impl_entity_soft_delete! {
    kind: EntityKind::God,
    dir: "gods",
    repo: god_repo,
    frontmatter: GodFrontmatter,
    store_load: load_god,
    store_save: save_god,
    store_delete: delete_god,
    row_from_frontmatter: god_row_from_frontmatter,
    soft_delete_fn: soft_delete_god,
    restore_fn: restore_god,
}
impl_entity_soft_delete! {
    kind: EntityKind::Dungeon,
    dir: "dungeons",
    repo: dungeon_repo,
    frontmatter: DungeonFrontmatter,
    store_load: load_dungeon,
    store_save: save_dungeon,
    store_delete: delete_dungeon,
    row_from_frontmatter: dungeon_row_from_frontmatter,
    soft_delete_fn: soft_delete_dungeon,
    restore_fn: restore_dungeon,
}

/// Record an entity's recovery row (the serialized TOML draft) before any
/// destructive step. The vault is never touched — the recovery payload is the
/// draft itself, so there is no trash file to move. The caller removes the draft
/// + DB row only after this returns (P1.3 safe order).
#[allow(clippy::too_many_arguments)]
async fn record_soft_delete(
    state: &AppState,
    kind: EntityKind,
    id: &str,
    slug: &str,
    name: &str,
    raw_vault_path: &str,
    payload_json: String,
    now: &str,
) -> Result<(), String> {
    let database = state.database();
    let soft_delete_row = db::SoftDeleteRow {
        id: 0,
        entity_type: kind.as_str().to_string(),
        entity_id: id.to_string(),
        name: name.to_string(),
        slug: slug.to_string(),
        original_vault_path: normalize_relative_path_for_storage(raw_vault_path),
        // No vault file is moved on delete, so there is no trash path.
        trash_vault_path: String::new(),
        payload_json,
        created_at: now.to_string(),
        undone_at: None,
        operation: "delete".to_string(),
    };
    state
        .soft_delete_repo()
        .insert(database.as_ref(), &soft_delete_row)
        .await?;
    Ok(())
}

/// Resolve the slug + vault path to restore to without touching the vault: keep
/// the originals unless a new entity claimed the slug's TOML draft or the
/// document-index path while this one was deleted, in which case mint fresh
/// unique ones.
async fn restore_collision(
    state: &AppState,
    store: &EntityStore,
    dir: &str,
    slug: &str,
    name: &str,
    vault_path: &str,
) -> Result<(String, String), String> {
    let database = state.database();
    let document_repo = state.document_repo();
    let restored_vault_path = normalize_relative_path_for_storage(vault_path);
    let toml_taken = store.root().join(dir).join(format!("{slug}.toml")).exists();
    let path_taken = document_repo
        .find_by_vault_path(database.as_ref(), &restored_vault_path)
        .await?
        .is_some();
    if toml_taken || path_taken {
        let fresh_slug = unique_slug_for_dir_with_ext(store.root(), dir, slug, "toml");
        let fresh_path =
            unique_readable_vault_path(document_repo.as_ref(), database.as_ref(), dir, name, None)
                .await?;
        Ok((fresh_slug, fresh_path))
    } else {
        Ok((slug.to_string(), restored_vault_path))
    }
}

/// Re-index a restored entity and mark its recovery row undone.
#[allow(clippy::too_many_arguments)]
async fn commit_restore(
    state: &AppState,
    kind: EntityKind,
    slug: &str,
    name: &str,
    vault_path: &str,
    created_at: &str,
    updated_at: &str,
    soft_delete_id: i64,
    now: &str,
) -> Result<(), String> {
    let database = state.database();
    state
        .document_repo()
        .upsert_index(
            database.as_ref(),
            kind.as_str(),
            slug,
            Some(name),
            vault_path,
            created_at,
            updated_at,
        )
        .await?;
    state
        .soft_delete_repo()
        .mark_undone(database.as_ref(), soft_delete_id, now)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct SoftDeleteEntityInput {
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SoftDeleteEntityResult {
    pub entity_type: EntityKind,
    pub id: String,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UndoSoftDeleteResult {
    pub entity_type: EntityKind,
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

/// Recovery record for a `publish` soft-delete. The full entity data is restored
/// from the canonical store on undo, so this only needs identifying fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PublishPayload {
    id: String,
    slug: String,
    name: String,
    vault_path: String,
}
