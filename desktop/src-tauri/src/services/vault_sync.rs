use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use dnd_core::config::load_effective;
use dnd_core::entity_store::EntityStore;
use dnd_core::npc::{FactionFrontmatter, ItemFrontmatter, LocationFrontmatter, NpcFrontmatter, normalize_markdown_file_stem};
use dnd_core::serialization::{carrying_to_db_text, exports_to_db_text, faction_list_to_db_text};
use dnd_core::vault::Vault;

use crate::app_state::AppState;
use crate::repositories::{
    db, DocumentRepository, FactionRepository, ItemRepository, LocationRepository, NpcRepository,
};
use crate::utils::normalize_relative_path_for_storage;

pub struct VaultSyncService;

impl VaultSyncService {
    pub async fn sync_from_vault(&self, state: &AppState) -> Result<(), String> {
        let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
        if !loaded.effective.vault.autoscan_on_start {
            return Ok(());
        }

        let Some(vault_path) = loaded.effective.vault.path.clone() else {
            return Ok(());
        };

        let vault = Vault::new(vault_path);
        vault.ensure_structure().map_err(|err| err.to_string())?;

        let store = EntityStore::new(&state.workspace_root).map_err(|err| err.to_string())?;
        let database = state.database();
        let npc_repo = state.npc_repo();
        let location_repo = state.location_repo();
        let faction_repo = state.faction_repo();
        let item_repo = state.item_repo();
        let document_repo = state.document_repo();

        sync_npcs(&store, database.as_ref(), npc_repo.as_ref(), document_repo.as_ref()).await?;
        sync_locations(&store, database.as_ref(), location_repo.as_ref(), document_repo.as_ref()).await?;
        sync_factions(&store, database.as_ref(), faction_repo.as_ref(), document_repo.as_ref()).await?;
        sync_items(&store, database.as_ref(), item_repo.as_ref(), document_repo.as_ref()).await?;

        Ok(())
    }
}

async fn sync_npcs(
    store: &EntityStore,
    database: &db::Database,
    npc_repo: &dyn NpcRepository,
    document_repo: &dyn DocumentRepository,
) -> Result<(), String> {
    let frontmatters = store
        .list_npcs()
        .map_err(|err| err.to_string())?;
    let mut synced_ids = HashSet::new();

    for frontmatter in frontmatters {
        let row = npc_row_from_frontmatter(&frontmatter)?;
        synced_ids.insert(row.id.clone());
        npc_repo.upsert(database, &row).await?;
        document_repo
            .upsert_index(
                database,
                "npc",
                &row.slug,
                Some(&row.name),
                &row.vault_path,
                &row.created_at,
                &row.updated_at,
            )
            .await?;
    }

    let existing = npc_repo.list_all(database).await?;
    for row in existing {
        if !synced_ids.contains(&row.id) {
            npc_repo.delete_by_id(database, &row.id).await?;
            document_repo
                .delete_by_vault_path(database, &row.vault_path)
                .await?;
        }
    }

    Ok(())
}

async fn sync_locations(
    store: &EntityStore,
    database: &db::Database,
    location_repo: &dyn LocationRepository,
    document_repo: &dyn DocumentRepository,
) -> Result<(), String> {
    let frontmatters = store
        .list_locations()
        .map_err(|err| err.to_string())?;
    let mut synced_ids = HashSet::new();

    for frontmatter in frontmatters {
        let row = location_row_from_frontmatter(&frontmatter)?;
        synced_ids.insert(row.id.clone());
        location_repo.upsert(database, &row).await?;
        document_repo
            .upsert_index(
                database,
                "location",
                &row.slug,
                Some(&row.name),
                &row.vault_path,
                &row.created_at,
                &row.updated_at,
            )
            .await?;
    }

    let existing = location_repo.list_all(database).await?;
    for row in existing {
        if !synced_ids.contains(&row.id) {
            location_repo.delete_by_id(database, &row.id).await?;
            document_repo
                .delete_by_vault_path(database, &row.vault_path)
                .await?;
        }
    }

    Ok(())
}

async fn sync_factions(
    store: &EntityStore,
    database: &db::Database,
    faction_repo: &dyn FactionRepository,
    document_repo: &dyn DocumentRepository,
) -> Result<(), String> {
    let frontmatters = store
        .list_factions()
        .map_err(|err| err.to_string())?;
    let mut synced_ids = HashSet::new();

    for frontmatter in frontmatters {
        let row = faction_row_from_frontmatter(&frontmatter)?;
        synced_ids.insert(row.id.clone());
        faction_repo.upsert(database, &row).await?;
        document_repo
            .upsert_index(
                database,
                "faction",
                &row.slug,
                Some(&row.name),
                &row.vault_path,
                &row.created_at,
                &row.updated_at,
            )
            .await?;
    }

    let existing = faction_repo.list_all(database).await?;
    for row in existing {
        if !synced_ids.contains(&row.id) {
            faction_repo.delete_by_id(database, &row.id).await?;
            document_repo
                .delete_by_vault_path(database, &row.vault_path)
                .await?;
        }
    }

    Ok(())
}

async fn sync_items(
    store: &EntityStore,
    database: &db::Database,
    item_repo: &dyn ItemRepository,
    document_repo: &dyn DocumentRepository,
) -> Result<(), String> {
    let frontmatters = store
        .list_items()
        .map_err(|err| err.to_string())?;
    let mut synced_ids = HashSet::new();

    for frontmatter in frontmatters {
        let row = item_row_from_frontmatter(&frontmatter)?;
        synced_ids.insert(row.id.clone());
        item_repo.upsert(database, &row).await?;
        document_repo
            .upsert_index(
                database,
                "item",
                &row.slug,
                Some(&row.name),
                &row.vault_path,
                &row.created_at,
                &row.updated_at,
            )
            .await?;
    }

    let existing = item_repo.list_all(database).await?;
    for row in existing {
        if !synced_ids.contains(&row.id) {
            item_repo.delete_by_id(database, &row.id).await?;
            document_repo
                .delete_by_vault_path(database, &row.vault_path)
                .await?;
        }
    }

    Ok(())
}

fn npc_row_from_frontmatter(frontmatter: &NpcFrontmatter) -> Result<db::NpcRow, String> {
    Ok(db::NpcRow {
        id: frontmatter.id.clone(),
        slug: frontmatter.slug.clone(),
        name: frontmatter.name.clone(),
        race: frontmatter.race.clone(),
        occupation: frontmatter.occupation.clone(),
        sex: frontmatter.sex.clone(),
        age: frontmatter.age.clone(),
        height: frontmatter.height.clone(),
        weight_lbs: frontmatter.weight_lbs.clone(),
        background: frontmatter.background.clone(),
        want_need: frontmatter.want_need.clone(),
        secret_obstacle: frontmatter.secret_obstacle.clone(),
        carrying: carrying_to_db_text(&frontmatter.carrying)
            .map_err(|err| err.to_string())?,
        location: frontmatter.location.clone(),
        vault_path: frontmatter.vault_path.clone(),
        created_at: frontmatter.created_at.clone(),
        updated_at: frontmatter.updated_at.clone(),
    })
}

fn location_row_from_frontmatter(
    frontmatter: &LocationFrontmatter,
) -> Result<db::LocationRow, String> {
    Ok(db::LocationRow {
        id: frontmatter.id.clone(),
        slug: frontmatter.slug.clone(),
        name: frontmatter.name.clone(),
        vault_path: frontmatter.vault_path.clone(),
        kind_type: frontmatter.kind_type.clone(),
        kind_custom: frontmatter.kind_custom.clone(),
        visual_description: frontmatter.visual_description.clone(),
        history_background: frontmatter.history_background.clone(),
        exports: exports_to_db_text(&frontmatter.exports).map_err(|err| err.to_string())?,
        tone: frontmatter.tone.clone(),
        authority: frontmatter.authority.clone(),
        danger_level: frontmatter.danger_level.clone(),
        current_tension: frontmatter.current_tension.clone(),
        created_at: frontmatter.created_at.clone(),
        updated_at: frontmatter.updated_at.clone(),
    })
}

fn faction_row_from_frontmatter(
    frontmatter: &FactionFrontmatter,
) -> Result<db::FactionRow, String> {
    Ok(db::FactionRow {
        id: frontmatter.id.clone(),
        slug: frontmatter.slug.clone(),
        name: frontmatter.name.clone(),
        vault_path: frontmatter.vault_path.clone(),
        kind_type: frontmatter.kind_type.clone(),
        kind_custom: frontmatter.kind_custom.clone(),
        public_description: frontmatter.public_description.clone(),
        true_agenda: frontmatter.true_agenda.clone(),
        methods: frontmatter.methods.clone(),
        leadership: frontmatter.leadership.clone(),
        headquarters: frontmatter.headquarters.clone(),
        sphere_of_influence: frontmatter.sphere_of_influence.clone(),
        resources_assets: frontmatter.resources_assets.clone(),
        allies: faction_list_to_db_text(&frontmatter.allies).map_err(|err| err.to_string())?,
        rivals_enemies: faction_list_to_db_text(&frontmatter.rivals_enemies)
            .map_err(|err| err.to_string())?,
        reputation: frontmatter.reputation.clone(),
        current_tension: frontmatter.current_tension.clone(),
        goals_short_term: faction_list_to_db_text(&frontmatter.goals_short_term)
            .map_err(|err| err.to_string())?,
        goals_long_term: faction_list_to_db_text(&frontmatter.goals_long_term)
            .map_err(|err| err.to_string())?,
        symbol_description: frontmatter.symbol_description.clone(),
        created_at: frontmatter.created_at.clone(),
        updated_at: frontmatter.updated_at.clone(),
    })
}

fn item_row_from_frontmatter(frontmatter: &ItemFrontmatter) -> Result<db::ItemRow, String> {
    Ok(db::ItemRow {
        id: frontmatter.id.clone(),
        slug: frontmatter.slug.clone(),
        name: frontmatter.name.clone(),
        vault_path: frontmatter.vault_path.clone(),
        category: frontmatter.category.clone(),
        rarity: frontmatter.rarity.clone(),
        attunement: frontmatter.attunement.clone(),
        materials: faction_list_to_db_text(&frontmatter.materials).map_err(|err| err.to_string())?,
        appearance: frontmatter.appearance.clone(),
        abilities: frontmatter.abilities.clone(),
        drawbacks: frontmatter.drawbacks.clone(),
        history: frontmatter.history.clone(),
        value: frontmatter.value.clone(),
        location: frontmatter.location.clone(),
        created_at: frontmatter.created_at.clone(),
        updated_at: frontmatter.updated_at.clone(),
    })
}

pub fn unique_trash_path(
    vault: &Vault,
    entity_dir: &str,
    slug: &str,
    timestamp: &str,
) -> Result<String, String> {
    let base = format!("{}-{}", slug, timestamp.replace(':', "").replace('-', ""));
    let mut candidate = format!(".trash/{entity_dir}/{base}.md");
    let mut index = 2;

    loop {
        let full = vault
            .resolve_relative(&PathBuf::from(&candidate))
            .map_err(|err| err.to_string())?;
        if !full.exists() {
            return Ok(candidate);
        }
        candidate = format!(".trash/{entity_dir}/{base}-{index}.md");
        index += 1;
    }
}

pub fn move_vault_file(
    vault: &Vault,
    source_relative: &str,
    target_relative: &str,
) -> Result<(), String> {
    let source_relative = normalize_relative_path_for_storage(source_relative);
    let target_relative = normalize_relative_path_for_storage(target_relative);
    let source_full = vault
        .resolve_relative(&PathBuf::from(&source_relative))
        .map_err(|err| err.to_string())?;
    if !source_full.exists() {
        return Err(format!(
            "source file does not exist: {}",
            source_full.display()
        ));
    }

    let target_full = vault
        .resolve_relative(&PathBuf::from(&target_relative))
        .map_err(|err| err.to_string())?;
    if let Some(parent) = target_full.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create trash directory {}: {}", parent.display(), err))?;
    }

    fs::rename(&source_full, &target_full).map_err(|err| {
        format!(
            "failed to move file from {} to {}: {}",
            source_full.display(),
            target_full.display(),
            err
        )
    })
}

pub fn unique_markdown_path_for_name(
    vault: &Vault,
    relative_dir: &str,
    display_name: &str,
    keep_path: Option<&str>,
) -> Result<String, String> {
    let base = normalize_markdown_file_stem(display_name);
    let mut candidate = base.clone();
    let mut index = 2;

    loop {
        let relative = PathBuf::from(relative_dir)
            .join(format!("{candidate}.md"))
            .to_string_lossy()
            .to_string();
        let relative = normalize_relative_path_for_storage(&relative);

        if keep_path.is_some_and(|existing| existing == relative) {
            return Ok(relative);
        }

        let full = vault
            .resolve_relative(&PathBuf::from(&relative))
            .map_err(|err| err.to_string())?;
        if !full.exists() {
            return Ok(relative);
        }

        candidate = format!("{base} {index}");
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{location_row_from_frontmatter, npc_row_from_frontmatter};
    use runebound_models::{LocationFrontmatter, NpcFrontmatter};

    #[test]
    fn npc_row_from_frontmatter_serializes_carrying() {
        let frontmatter = NpcFrontmatter {
            doc_type: "npc".to_string(),
            id: "npc_1".to_string(),
            slug: "lirael".to_string(),
            name: "Lirael".to_string(),
            vault_path: "npcs/Lirael Drake.md".to_string(),
            race: "Elf".to_string(),
            occupation: "Archivist".to_string(),
            sex: "female".to_string(),
            age: "133".to_string(),
            height: "5'9\"".to_string(),
            weight_lbs: "140".to_string(),
            background: "Raised in the argent library.".to_string(),
            want_need: "Safeguard scrolls.".to_string(),
            secret_obstacle: "Cursed dreams".to_string(),
            carrying: vec!["Silver quill".to_string()],
            location: "Silversong".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T12:00:00Z".to_string(),
        };

        let row = npc_row_from_frontmatter(&frontmatter).expect("row");
        assert_eq!(row.carrying, "[\"Silver quill\"]");
        assert_eq!(row.slug, "lirael");
        assert_eq!(row.vault_path, "npcs/Lirael Drake.md");
    }

    #[test]
    fn location_row_from_frontmatter_serializes_exports() {
        let frontmatter = LocationFrontmatter {
            doc_type: "location".to_string(),
            id: "loc_1".to_string(),
            slug: "silkenhollow".to_string(),
            name: "Silkenhollow".to_string(),
            vault_path: "locations/silkenhollow.md".to_string(),
            kind_type: "other".to_string(),
            kind_custom: Some("Sanctum".to_string()),
            visual_description: "Quiet grove".to_string(),
            history_background: "Forgotten".to_string(),
            exports: vec!["Incense".to_string(), "Silk".to_string()],
            tone: "Calm".to_string(),
            authority: "Circle".to_string(),
            danger_level: "low".to_string(),
            current_tension: "None".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T12:00:00Z".to_string(),
        };

        let row = location_row_from_frontmatter(&frontmatter).expect("row");
        assert!(row.exports.contains("Incense"));
        assert_eq!(row.kind_custom.as_deref(), Some("Sanctum"));
    }
}
