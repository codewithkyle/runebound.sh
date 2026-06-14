use std::fs;
use std::path::PathBuf;

use dnd_core::config::{load_effective, validate_for_runtime};
use dnd_core::npc::{
    FactionFrontmatter, LocationFrontmatter, NpcFrontmatter, UNKNOWN_LOCATION, merge_runebound_block,
    now_timestamp, render_faction_markdown, render_location_markdown, render_npc_markdown, slugify,
    unique_slug_for_dir,
};
use dnd_core::vault::Vault;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::repositories::db;
use crate::services::ai_generation::LocationSeed;
use crate::services::vault_sync::{read_vault_file_if_exists, unique_markdown_path_for_name};
use crate::utils::{
    normalize_exports, normalize_faction_kind_type, normalize_location_danger_level,
    normalize_location_kind_type, normalize_relative_path_for_storage, normalize_sex,
    normalize_unknown_list, normalize_unknown_text, validate_location_details,
};
use crate::{carrying_to_db_text, exports_to_db_text, faction_list_to_db_text};

pub struct EntityPersistenceService;

impl EntityPersistenceService {
    pub async fn save_npc_draft(
        &self,
        input: SaveNpcDraftInput,
        state: &AppState,
    ) -> Result<SaveNpcDraftResult, String> {
        if input.id.trim().is_empty() {
            return Err("npc id cannot be empty".to_string());
        }

        let name = input.name.trim();
        if name.is_empty() {
            return Err("npc name cannot be empty".to_string());
        }
        let race = input.race.trim();
        if race.is_empty() {
            return Err("npc race cannot be empty".to_string());
        }

        let occupation = normalize_unknown_text(&input.occupation);
        let sex = normalize_sex(&input.sex)?;
        let age = normalize_unknown_text(&input.age);
        let height = normalize_unknown_text(&input.height);
        let weight_lbs = normalize_unknown_text(&input.weight_lbs);
        let background = normalize_unknown_text(&input.background);
        let want_need = normalize_unknown_text(&input.want_need);
        let secret_obstacle = normalize_unknown_text(&input.secret_obstacle);
        let carrying = normalize_unknown_list(input.carrying);
        let carrying_db = carrying_to_db_text(&carrying)?;
        let location = if input.location.trim().is_empty() {
            UNKNOWN_LOCATION.to_string()
        } else {
            input.location.trim().to_string()
        };

        let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
        validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
        let vault_path = loaded
            .effective
            .vault
            .path
            .clone()
            .ok_or_else(|| "vault.path is not configured".to_string())?;
        let vault = Vault::new(vault_path);
        vault.ensure_structure().map_err(|err| err.to_string())?;

        let database = state.database();
        let npc_repo = state.npc_repo();
        let document_repo = state.document_repo();
        let now = now_timestamp();

        let existing = npc_repo
            .find_by_id(database.as_ref(), input.id.trim())
            .await?;

        let (slug, relative_path, created_at, previous_path) = if let Some(current) = existing {
            let current_vault_path = normalize_relative_path_for_storage(&current.vault_path);
            let desired_base_slug = slugify(name);
            let desired_path = unique_markdown_path_for_name(
                &vault,
                "npcs",
                name,
                Some(current_vault_path.as_str()),
            )?;

            if desired_base_slug == current.slug {
                (
                    current.slug,
                    desired_path.clone(),
                    current.created_at,
                    if desired_path == current_vault_path {
                        None
                    } else {
                        Some(current_vault_path)
                    },
                )
            } else {
                let next_slug = unique_slug_for_dir(vault.root(), "npcs", &desired_base_slug);
                (
                    next_slug,
                    desired_path,
                    current.created_at,
                    Some(current_vault_path),
                )
            }
        } else {
            let base_slug = slugify(name);
            let slug = unique_slug_for_dir(vault.root(), "npcs", &base_slug);
            (
                slug.clone(),
                unique_markdown_path_for_name(&vault, "npcs", name, None)?,
                now.clone(),
                None,
            )
        };

        let markdown = render_npc_markdown(&NpcFrontmatter {
            doc_type: "npc".to_string(),
            id: input.id.trim().to_string(),
            slug: slug.clone(),
            name: name.to_string(),
            race: race.to_string(),
            occupation: occupation.clone(),
            sex: sex.clone(),
            age: age.clone(),
            height: height.clone(),
            weight_lbs: weight_lbs.clone(),
            background: background.clone(),
            want_need: want_need.clone(),
            secret_obstacle: secret_obstacle.clone(),
            carrying: carrying.clone(),
            location: location.clone(),
            created_at: created_at.clone(),
            updated_at: now.clone(),
        })
        .map_err(|err| err.to_string())?;

        let existing_markdown = if let Some(ref old_path) = previous_path {
            if old_path != &relative_path {
                match read_vault_file_if_exists(&vault, old_path) {
                    Ok(Some(contents)) => Some(contents),
                    Ok(None) => read_vault_file_if_exists(&vault, &relative_path)?,
                    Err(err) => return Err(err),
                }
            } else {
                read_vault_file_if_exists(&vault, &relative_path)?
            }
        } else {
            read_vault_file_if_exists(&vault, &relative_path)?
        };
        let merged_markdown = match existing_markdown {
            Some(existing) => merge_runebound_block(&existing, &markdown),
            None => markdown,
        };

        vault
            .write_relative(&PathBuf::from(&relative_path), &merged_markdown)
            .map_err(|err| err.to_string())?;

        let npc_row = db::NpcRow {
            id: input.id.trim().to_string(),
            slug: slug.clone(),
            name: name.to_string(),
            race: race.to_string(),
            occupation,
            sex,
            age,
            height,
            weight_lbs,
            background,
            want_need,
            secret_obstacle,
            carrying: carrying_db,
            location,
            vault_path: relative_path.clone(),
            created_at: created_at.clone(),
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

        if let Some(old_path) = previous_path {
            if old_path != npc_row.vault_path {
                document_repo
                    .delete_by_vault_path(database.as_ref(), &old_path)
                    .await?;

                if let Ok(old_full_path) = vault.resolve_relative(&PathBuf::from(&old_path)) {
                    if old_full_path.exists() {
                        fs::remove_file(&old_full_path).map_err(|err| {
                            format!(
                                "failed to remove old npc file {}: {}",
                                old_full_path.display(),
                                err
                            )
                        })?;
                    }
                }
            }
        }

        Ok(SaveNpcDraftResult {
            id: npc_row.id,
            slug: npc_row.slug,
            vault_path: npc_row.vault_path,
            created_at: npc_row.created_at,
            updated_at: npc_row.updated_at,
        })
    }

    pub async fn save_location_draft(
        &self,
        input: SaveLocationDraftInput,
        state: &AppState,
    ) -> Result<SaveLocationDraftResult, String> {
        if input.id.trim().is_empty() {
            return Err("location id cannot be empty".to_string());
        }

        let name = input.name.trim();
        if name.is_empty() {
            return Err("location name cannot be empty".to_string());
        }

        let _legacy_slug_input = input.slug.trim();
        let previous_vault_path_input = normalize_relative_path_for_storage(input.vault_path.trim());

        let kind_type = normalize_location_kind_type(&input.kind_type)?;
        let mut kind_custom = input.kind_custom.map(|value| normalize_unknown_text(&value));
        if kind_type == "other" {
            if kind_custom
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
            {
                return Err("kind_custom is required when kind_type is other".to_string());
            }
        } else {
            kind_custom = None;
        }
        let visual_description = normalize_unknown_text(&input.visual_description);
        let history_background = normalize_unknown_text(&input.history_background);
        let exports = normalize_exports(input.exports);
        let tone = normalize_unknown_text(&input.tone);
        let authority = normalize_unknown_text(&input.authority);
        let danger_level = normalize_location_danger_level(&input.danger_level)?;
        let current_tension = normalize_unknown_text(&input.current_tension);

        validate_location_details(&LocationSeed {
            name: name.to_string(),
            kind_type: kind_type.clone(),
            kind_custom: kind_custom.clone(),
            visual_description: visual_description.clone(),
            history_background: history_background.clone(),
            exports: exports.clone(),
            tone: tone.clone(),
            authority: authority.clone(),
            danger_level: danger_level.clone(),
            current_tension: current_tension.clone(),
        })?;
        let exports_db = exports_to_db_text(&exports)?;

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
        let location_repo = state.location_repo();
        let document_repo = state.document_repo();
        let now = now_timestamp();
        let existing = location_repo
            .find_by_id(database.as_ref(), input.id.trim())
            .await?;

        let (slug, relative_path, created_at, previous_path) = if let Some(current) = existing {
            let current_vault_path = normalize_relative_path_for_storage(&current.vault_path);
            let desired_base_slug = slugify(name);
            let desired_path = unique_markdown_path_for_name(
                &vault,
                "locations",
                name,
                Some(current_vault_path.as_str()),
            )?;

            if desired_base_slug == current.slug {
                (
                    current.slug,
                    desired_path.clone(),
                    current.created_at,
                    if desired_path == current_vault_path {
                        None
                    } else {
                        Some(current_vault_path)
                    },
                )
            } else {
                (
                    unique_slug_for_dir(vault.root(), "locations", &desired_base_slug),
                    desired_path,
                    current.created_at,
                    Some(current_vault_path),
                )
            }
        } else {
            let base_slug = slugify(name);
            (
                unique_slug_for_dir(vault.root(), "locations", &base_slug),
                unique_markdown_path_for_name(&vault, "locations", name, None)?,
                now.clone(),
                if previous_vault_path_input.is_empty() {
                    None
                } else {
                    Some(previous_vault_path_input.to_string())
                },
            )
        };

        let markdown = render_location_markdown(&LocationFrontmatter {
            doc_type: "location".to_string(),
            id: input.id.trim().to_string(),
            slug: slug.clone(),
            name: name.to_string(),
            kind_type: kind_type.clone(),
            kind_custom: kind_custom.clone(),
            visual_description: visual_description.clone(),
            history_background: history_background.clone(),
            exports: exports.clone(),
            tone: tone.clone(),
            authority: authority.clone(),
            danger_level: danger_level.clone(),
            current_tension: current_tension.clone(),
            created_at: created_at.clone(),
            updated_at: now.clone(),
        })
        .map_err(|err| err.to_string())?;

        let existing_markdown = if let Some(ref old_path) = previous_path {
            if old_path != &relative_path {
                match read_vault_file_if_exists(&vault, old_path) {
                    Ok(Some(contents)) => Some(contents),
                    Ok(None) => read_vault_file_if_exists(&vault, &relative_path)?,
                    Err(err) => return Err(err),
                }
            } else {
                read_vault_file_if_exists(&vault, &relative_path)?
            }
        } else {
            read_vault_file_if_exists(&vault, &relative_path)?
        };
        let merged_markdown = match existing_markdown {
            Some(existing) => merge_runebound_block(&existing, &markdown),
            None => markdown,
        };

        vault
            .write_relative(&PathBuf::from(&relative_path), &merged_markdown)
            .map_err(|err| err.to_string())?;

        let location_row = db::LocationRow {
            id: input.id.trim().to_string(),
            slug,
            name: name.to_string(),
            vault_path: relative_path.clone(),
            kind_type,
            kind_custom,
            visual_description,
            history_background,
            exports: exports_db,
            tone,
            authority,
            danger_level,
            current_tension,
            created_at: created_at.clone(),
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

        if let Some(old_path) = previous_path {
            if old_path != location_row.vault_path {
                document_repo
                    .delete_by_vault_path(database.as_ref(), &old_path)
                    .await?;

                if let Ok(old_full_path) = vault.resolve_relative(&PathBuf::from(&old_path)) {
                    if old_full_path.exists() {
                        fs::remove_file(&old_full_path).map_err(|err| {
                            format!(
                                "failed to remove old location file {}: {}",
                                old_full_path.display(),
                                err
                            )
                        })?;
                    }
                }
            }
        }

        Ok(SaveLocationDraftResult {
            id: location_row.id,
            slug: location_row.slug,
            vault_path: location_row.vault_path,
            created_at: location_row.created_at,
            updated_at: location_row.updated_at,
        })
    }

    pub async fn save_faction_draft(
        &self,
        input: SaveFactionDraftInput,
        state: &AppState,
    ) -> Result<SaveFactionDraftResult, String> {
        if input.id.trim().is_empty() {
            return Err("faction id cannot be empty".to_string());
        }
        if input.name.trim().is_empty() {
            return Err("faction name cannot be empty".to_string());
        }

        let kind_type = normalize_faction_kind_type(&input.kind_type)?;
        let kind_custom = if kind_type == "other" {
            let value = input
                .kind_custom
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "kind_custom is required when kind_type is other".to_string())?;
            Some(value.to_string())
        } else {
            None
        };

        let public_description = normalize_unknown_text(&input.public_description);
        let true_agenda = normalize_unknown_text(&input.true_agenda);
        let methods = normalize_unknown_text(&input.methods);
        let leadership = normalize_unknown_text(&input.leadership);
        let headquarters = normalize_unknown_text(&input.headquarters);
        let sphere_of_influence = normalize_unknown_text(&input.sphere_of_influence);
        let resources_assets = normalize_unknown_text(&input.resources_assets);
        let allies = normalize_unknown_list(input.allies);
        let rivals_enemies = normalize_unknown_list(input.rivals_enemies);
        let reputation = normalize_unknown_text(&input.reputation);
        let current_tension = normalize_unknown_text(&input.current_tension);
        let goals_short_term = normalize_unknown_list(input.goals_short_term);
        let goals_long_term = normalize_unknown_list(input.goals_long_term);
        let symbol_description = normalize_unknown_text(&input.symbol_description);

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
        let faction_repo = state.faction_repo();
        let document_repo = state.document_repo();
        let now = now_timestamp();
        let existing = faction_repo
            .find_by_id(database.as_ref(), input.id.trim())
            .await?;

        let provided_relative_path = normalize_relative_path_for_storage(input.vault_path.trim());
        let previous_path = if let Some(row) = existing.as_ref() {
            Some(normalize_relative_path_for_storage(&row.vault_path))
        } else if !provided_relative_path.is_empty() {
            Some(provided_relative_path)
        } else {
            None
        };
        let created_at = existing
            .as_ref()
            .map(|row| row.created_at.clone())
            .unwrap_or_else(|| now.clone());

        let provided_slug = input.slug.trim();
        let base_slug = if provided_slug.is_empty() {
            slugify(&input.name)
        } else {
            slugify(provided_slug)
        };
        let slug = if let Some(row) = existing.as_ref() {
            if row.slug != base_slug {
                base_slug
            } else {
                row.slug.clone()
            }
        } else {
            unique_slug_for_dir(vault.root(), "factions", &base_slug)
        };

        let desired_path = unique_markdown_path_for_name(
            &vault,
            "factions",
            &input.name,
            previous_path.as_deref(),
        )?;

        let frontmatter = FactionFrontmatter {
            doc_type: "faction".to_string(),
            id: input.id.trim().to_string(),
            slug: slug.clone(),
            name: input.name.trim().to_string(),
            kind_type: kind_type.clone(),
            kind_custom: kind_custom.clone(),
            public_description: public_description.clone(),
            true_agenda: true_agenda.clone(),
            methods: methods.clone(),
            leadership: leadership.clone(),
            headquarters: headquarters.clone(),
            sphere_of_influence: sphere_of_influence.clone(),
            resources_assets: resources_assets.clone(),
            allies: allies.clone(),
            rivals_enemies: rivals_enemies.clone(),
            reputation: reputation.clone(),
            current_tension: current_tension.clone(),
            goals_short_term: goals_short_term.clone(),
            goals_long_term: goals_long_term.clone(),
            symbol_description: symbol_description.clone(),
            created_at: created_at.clone(),
            updated_at: now.clone(),
        };

        let runebound_block = render_faction_markdown(&frontmatter).map_err(|err| err.to_string())?;
        let existing_file = read_vault_file_if_exists(&vault, &desired_path)?;
        let content = if let Some(current) = existing_file {
            merge_runebound_block(&current, &runebound_block)
        } else {
            runebound_block
        };
        vault
            .write_relative(&PathBuf::from(&desired_path), &content)
            .map_err(|err| err.to_string())?;

        let allies_db = faction_list_to_db_text(&allies)?;
        let rivals_db = faction_list_to_db_text(&rivals_enemies)?;
        let goals_short_db = faction_list_to_db_text(&goals_short_term)?;
        let goals_long_db = faction_list_to_db_text(&goals_long_term)?;

        let faction_row = db::FactionRow {
            id: input.id.trim().to_string(),
            slug,
            name: input.name.trim().to_string(),
            vault_path: desired_path,
            kind_type,
            kind_custom,
            public_description,
            true_agenda,
            methods,
            leadership,
            headquarters,
            sphere_of_influence,
            resources_assets,
            allies: allies_db,
            rivals_enemies: rivals_db,
            reputation,
            current_tension,
            goals_short_term: goals_short_db,
            goals_long_term: goals_long_db,
            symbol_description,
            created_at: created_at.clone(),
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

        if let Some(old_path) = previous_path {
            if old_path != faction_row.vault_path {
                document_repo
                    .delete_by_vault_path(database.as_ref(), &old_path)
                    .await?;
                if let Ok(old_full_path) = vault.resolve_relative(&PathBuf::from(&old_path)) {
                    if old_full_path.exists() {
                        fs::remove_file(&old_full_path).map_err(|err| {
                            format!(
                                "failed to remove old faction file {}: {}",
                                old_full_path.display(),
                                err
                            )
                        })?;
                    }
                }
            }
        }

        Ok(SaveFactionDraftResult {
            id: faction_row.id,
            slug: faction_row.slug,
            vault_path: faction_row.vault_path,
            created_at: faction_row.created_at,
            updated_at: faction_row.updated_at,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveNpcDraftInput {
    pub id: String,
    pub name: String,
    pub race: String,
    pub occupation: String,
    pub sex: String,
    pub age: String,
    pub height: String,
    pub weight_lbs: String,
    pub background: String,
    pub want_need: String,
    pub secret_obstacle: String,
    pub carrying: Vec<String>,
    pub location: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveNpcDraftResult {
    pub id: String,
    pub slug: String,
    pub vault_path: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveLocationDraftInput {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub vault_path: String,
    pub kind_type: String,
    pub kind_custom: Option<String>,
    pub visual_description: String,
    pub history_background: String,
    pub exports: Vec<String>,
    pub tone: String,
    pub authority: String,
    pub danger_level: String,
    pub current_tension: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveLocationDraftResult {
    pub id: String,
    pub slug: String,
    pub vault_path: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveFactionDraftInput {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub kind_type: String,
    pub kind_custom: Option<String>,
    pub public_description: String,
    pub true_agenda: String,
    pub methods: String,
    pub leadership: String,
    pub headquarters: String,
    pub sphere_of_influence: String,
    pub resources_assets: String,
    pub allies: Vec<String>,
    pub rivals_enemies: Vec<String>,
    pub reputation: String,
    pub current_tension: String,
    pub goals_short_term: Vec<String>,
    pub goals_long_term: Vec<String>,
    pub symbol_description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveFactionDraftResult {
    pub id: String,
    pub slug: String,
    pub vault_path: String,
    pub created_at: String,
    pub updated_at: String,
}
