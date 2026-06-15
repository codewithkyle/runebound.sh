use dnd_core::entity_store::EntityStore;
use dnd_core::npc::{
    FactionFrontmatter, ItemFrontmatter, LocationFrontmatter, NpcFrontmatter, UNKNOWN_LOCATION,
    normalize_markdown_file_stem, now_timestamp, slugify, unique_slug_for_dir_with_ext,
};
use dnd_core::serialization::{
    carrying_to_db_text, exports_to_db_text, faction_list_to_db_text,
};
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::repositories::db;
use crate::services::ai_generation::LocationSeed;
use crate::utils::{
    normalize_exports, normalize_faction_kind_type, normalize_item_category, normalize_item_rarity,
    normalize_location_danger_level, normalize_location_kind_type, normalize_relative_path_for_storage,
    normalize_sex, normalize_unknown_list, normalize_unknown_text, validate_location_details,
};

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

        let store = EntityStore::new(&state.workspace_root).map_err(|err| err.to_string())?;
        let database = state.database();
        let npc_repo = state.npc_repo();
        let document_repo = state.document_repo();
        let now = now_timestamp();

        let existing = npc_repo
            .find_by_id(database.as_ref(), input.id.trim())
            .await?;

        let base_slug = slugify(name);
        let slug = if let Some(current) = existing.as_ref() {
            if current.slug == base_slug {
                current.slug.clone()
            } else {
                unique_slug_for_dir_with_ext(store.root(), "npcs", &base_slug, "toml")
            }
        } else {
            unique_slug_for_dir_with_ext(store.root(), "npcs", &base_slug, "toml")
        };

        let created_at = existing
            .as_ref()
            .map(|row| row.created_at.clone())
            .unwrap_or_else(|| now.clone());
        let vault_path = if let Some(current) = existing.as_ref() {
            let readable = self
                .unique_readable_vault_path(
                    document_repo.as_ref(),
                    database.as_ref(),
                    "npcs",
                    name,
                    Some(&current.slug),
                )
                .await?;
            if normalize_relative_path_for_storage(&current.vault_path)
                == normalize_relative_path_for_storage(&readable)
            {
                current.vault_path.clone()
            } else {
                document_repo
                    .delete_by_vault_path(database.as_ref(), &current.vault_path)
                    .await?;
                readable
            }
        } else {
            self.unique_readable_vault_path(
                document_repo.as_ref(),
                database.as_ref(),
                "npcs",
                name,
                None,
            )
            .await?
        };

        let published_at = match existing.as_ref() {
            Some(current) => store
                .load_npc(&current.slug)
                .ok()
                .flatten()
                .and_then(|prior| prior.published_at),
            None => None,
        };

        let frontmatter = NpcFrontmatter {
            doc_type: "npc".to_string(),
            id: input.id.trim().to_string(),
            slug: slug.clone(),
            name: name.to_string(),
            vault_path: vault_path.clone(),
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
            published_at,
        };

        store
            .save_npc(&frontmatter)
            .map_err(|err| err.to_string())?;
        if let Some(current) = existing.as_ref() {
            if current.slug != slug {
                store
                    .delete_npc(&current.slug)
                    .map_err(|err| err.to_string())?
            }
        }

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
            vault_path: vault_path.clone(),
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

        let store = EntityStore::new(&state.workspace_root).map_err(|err| err.to_string())?;
        let database = state.database();
        let location_repo = state.location_repo();
        let document_repo = state.document_repo();
        let now = now_timestamp();
        let existing = location_repo
            .find_by_id(database.as_ref(), input.id.trim())
            .await?;
        let base_slug = slugify(name);
        let slug = if let Some(current) = existing.as_ref() {
            if current.slug == base_slug {
                current.slug.clone()
            } else {
                unique_slug_for_dir_with_ext(store.root(), "locations", &base_slug, "toml")
            }
        } else {
            unique_slug_for_dir_with_ext(store.root(), "locations", &base_slug, "toml")
        };

        let created_at = existing
            .as_ref()
            .map(|row| row.created_at.clone())
            .unwrap_or_else(|| now.clone());
        let requested_path = normalize_relative_path_for_storage(input.vault_path.trim());
        let vault_path = if !requested_path.is_empty() {
            requested_path
        } else if let Some(current) = existing.as_ref() {
            let readable = self
                .unique_readable_vault_path(
                    document_repo.as_ref(),
                    database.as_ref(),
                    "locations",
                    name,
                    Some(&current.slug),
                )
                .await?;
            if normalize_relative_path_for_storage(&current.vault_path)
                == normalize_relative_path_for_storage(&readable)
            {
                current.vault_path.clone()
            } else {
                document_repo
                    .delete_by_vault_path(database.as_ref(), &current.vault_path)
                    .await?;
                readable
            }
        } else {
            self.unique_readable_vault_path(
                document_repo.as_ref(),
                database.as_ref(),
                "locations",
                name,
                None,
            )
            .await?
        };

        let published_at = match existing.as_ref() {
            Some(current) => store
                .load_location(&current.slug)
                .ok()
                .flatten()
                .and_then(|prior| prior.published_at),
            None => None,
        };

        let frontmatter = LocationFrontmatter {
            doc_type: "location".to_string(),
            id: input.id.trim().to_string(),
            slug: slug.clone(),
            name: name.to_string(),
            vault_path: vault_path.clone(),
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
            published_at,
        };

        store
            .save_location(&frontmatter)
            .map_err(|err| err.to_string())?;
        if let Some(current) = existing.as_ref() {
            if current.slug != slug {
                store
                    .delete_location(&current.slug)
                    .map_err(|err| err.to_string())?;
            }
        }

        let location_row = db::LocationRow {
            id: input.id.trim().to_string(),
            slug,
            name: name.to_string(),
            vault_path: vault_path.clone(),
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

        let store = EntityStore::new(&state.workspace_root).map_err(|err| err.to_string())?;
        let database = state.database();
        let faction_repo = state.faction_repo();
        let document_repo = state.document_repo();
        let now = now_timestamp();
        let existing = faction_repo
            .find_by_id(database.as_ref(), input.id.trim())
            .await?;
        let created_at = existing
            .as_ref()
            .map(|row| row.created_at.clone())
            .unwrap_or_else(|| now.clone());

        let desired_base_slug = slugify(&input.name);
        let slug = if let Some(row) = existing.as_ref() {
            if row.slug == desired_base_slug {
                row.slug.clone()
            } else {
                unique_slug_for_dir_with_ext(store.root(), "factions", &desired_base_slug, "toml")
            }
        } else {
            unique_slug_for_dir_with_ext(store.root(), "factions", &desired_base_slug, "toml")
        };

        let requested_path = normalize_relative_path_for_storage(input.vault_path.trim());
        let vault_path = if !requested_path.is_empty() {
            requested_path
        } else if let Some(row) = existing.as_ref() {
            let readable = self
                .unique_readable_vault_path(
                    document_repo.as_ref(),
                    database.as_ref(),
                    "factions",
                    input.name.trim(),
                    Some(&row.slug),
                )
                .await?;
            if normalize_relative_path_for_storage(&row.vault_path)
                == normalize_relative_path_for_storage(&readable)
            {
                row.vault_path.clone()
            } else {
                document_repo
                    .delete_by_vault_path(database.as_ref(), &row.vault_path)
                    .await?;
                readable
            }
        } else {
            self.unique_readable_vault_path(
                document_repo.as_ref(),
                database.as_ref(),
                "factions",
                input.name.trim(),
                None,
            )
            .await?
        };

        let published_at = match existing.as_ref() {
            Some(current) => store
                .load_faction(&current.slug)
                .ok()
                .flatten()
                .and_then(|prior| prior.published_at),
            None => None,
        };

        let frontmatter = FactionFrontmatter {
            doc_type: "faction".to_string(),
            id: input.id.trim().to_string(),
            slug: slug.clone(),
            name: input.name.trim().to_string(),
            vault_path: vault_path.clone(),
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
            published_at,
        };

        store
            .save_faction(&frontmatter)
            .map_err(|err| err.to_string())?;
        if let Some(current) = existing.as_ref() {
            if current.slug != slug {
                store
                    .delete_faction(&current.slug)
                    .map_err(|err| err.to_string())?;
            }
        }

        let allies_db = faction_list_to_db_text(&allies)?;
        let rivals_db = faction_list_to_db_text(&rivals_enemies)?;
        let goals_short_db = faction_list_to_db_text(&goals_short_term)?;
        let goals_long_db = faction_list_to_db_text(&goals_long_term)?;

        let faction_row = db::FactionRow {
            id: input.id.trim().to_string(),
            slug,
            name: input.name.trim().to_string(),
            vault_path: vault_path.clone(),
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

        Ok(SaveFactionDraftResult {
            id: faction_row.id,
            slug: faction_row.slug,
            vault_path: faction_row.vault_path,
            created_at: faction_row.created_at,
            updated_at: faction_row.updated_at,
        })
    }

    async fn unique_readable_vault_path(
        &self,
        document_repo: &dyn crate::repositories::DocumentRepository,
        database: &db::Database,
        relative_dir: &str,
        display_name: &str,
        current_slug: Option<&str>,
    ) -> Result<String, String> {
        let base = normalize_markdown_file_stem(display_name);
        let mut candidate = format!("{relative_dir}/{base}.md");
        let mut idx = 2;
        while let Some(found_slug) = document_repo
            .find_by_vault_path(database, &candidate)
            .await?
        {
            if current_slug == Some(found_slug.as_str()) {
                return Ok(candidate);
            }
            candidate = format!("{relative_dir}/{base} {idx}.md");
            idx += 1;
        }
        Ok(candidate)
    }

    pub async fn save_item_draft(
        &self,
        input: SaveItemDraftInput,
        state: &AppState,
    ) -> Result<SaveItemDraftResult, String> {
        if input.id.trim().is_empty() {
            return Err("item id cannot be empty".to_string());
        }

        let name = input.name.trim();
        if name.is_empty() {
            return Err("item name cannot be empty".to_string());
        }

        let category = normalize_item_category(&input.category)?;
        let rarity = normalize_item_rarity(&input.rarity)?;
        let attunement = normalize_unknown_text(&input.attunement);
        let materials = normalize_unknown_list(input.materials);
        let materials_db = faction_list_to_db_text(&materials)?;
        let appearance = normalize_unknown_text(&input.appearance);
        let abilities = normalize_unknown_text(&input.abilities);
        let drawbacks = normalize_unknown_text(&input.drawbacks);
        let history = normalize_unknown_text(&input.history);
        let value = normalize_unknown_text(&input.value);
        let location = normalize_unknown_text(&input.location);

        let store = EntityStore::new(&state.workspace_root).map_err(|err| err.to_string())?;
        let database = state.database();
        let item_repo = state.item_repo();
        let document_repo = state.document_repo();
        let now = now_timestamp();
        let existing = item_repo
            .find_by_id(database.as_ref(), input.id.trim())
            .await?;

        let desired_base_slug = slugify(name);
        let slug = if let Some(current) = existing.as_ref() {
            if current.slug == desired_base_slug {
                current.slug.clone()
            } else {
                unique_slug_for_dir_with_ext(store.root(), "items", &desired_base_slug, "toml")
            }
        } else {
            unique_slug_for_dir_with_ext(store.root(), "items", &desired_base_slug, "toml")
        };

        let created_at = existing
            .as_ref()
            .map(|row| row.created_at.clone())
            .unwrap_or_else(|| now.clone());
        let vault_path = if let Some(current) = existing.as_ref() {
            let readable = self
                .unique_readable_vault_path(
                    document_repo.as_ref(),
                    database.as_ref(),
                    "items",
                    name,
                    Some(&current.slug),
                )
                .await?;
            if normalize_relative_path_for_storage(&current.vault_path)
                == normalize_relative_path_for_storage(&readable)
            {
                current.vault_path.clone()
            } else {
                document_repo
                    .delete_by_vault_path(database.as_ref(), &current.vault_path)
                    .await?;
                readable
            }
        } else {
            self.unique_readable_vault_path(
                document_repo.as_ref(),
                database.as_ref(),
                "items",
                name,
                None,
            )
            .await?
        };

        let published_at = match existing.as_ref() {
            Some(current) => store
                .load_item(&current.slug)
                .ok()
                .flatten()
                .and_then(|prior| prior.published_at),
            None => None,
        };

        let frontmatter = ItemFrontmatter {
            doc_type: "item".to_string(),
            id: input.id.trim().to_string(),
            slug: slug.clone(),
            name: name.to_string(),
            vault_path: vault_path.clone(),
            category: category.clone(),
            rarity: rarity.clone(),
            attunement: attunement.clone(),
            materials: materials.clone(),
            appearance: appearance.clone(),
            abilities: abilities.clone(),
            drawbacks: drawbacks.clone(),
            history: history.clone(),
            value: value.clone(),
            location: location.clone(),
            created_at: created_at.clone(),
            updated_at: now.clone(),
            published_at,
        };

        store
            .save_item(&frontmatter)
            .map_err(|err| err.to_string())?;
        if let Some(current) = existing.as_ref() {
            if current.slug != slug {
                store
                    .delete_item(&current.slug)
                    .map_err(|err| err.to_string())?;
            }
        }

        let item_row = db::ItemRow {
            id: input.id.trim().to_string(),
            slug,
            name: name.to_string(),
            vault_path: vault_path.clone(),
            category,
            rarity,
            attunement,
            materials: materials_db,
            appearance,
            abilities,
            drawbacks,
            history,
            value,
            location,
            created_at: created_at.clone(),
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

        Ok(SaveItemDraftResult {
            id: item_row.id,
            slug: item_row.slug,
            vault_path: item_row.vault_path,
            created_at: item_row.created_at,
            updated_at: item_row.updated_at,
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

#[derive(Debug, Clone, Deserialize)]
pub struct SaveItemDraftInput {
    pub id: String,
    pub name: String,
    pub category: String,
    pub rarity: String,
    pub attunement: String,
    pub materials: Vec<String>,
    pub appearance: String,
    pub abilities: String,
    pub drawbacks: String,
    pub history: String,
    pub value: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveItemDraftResult {
    pub id: String,
    pub slug: String,
    pub vault_path: String,
    pub created_at: String,
    pub updated_at: String,
}
