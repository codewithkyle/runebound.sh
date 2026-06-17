use async_trait::async_trait;
use dnd_core::db as core_db;
use dnd_core::vault::Vault;
use std::path::PathBuf;

pub mod db {
    pub use dnd_core::db::{
        Database, DungeonRow, EventRow, FactionRow, GodRow, ItemRow, LocationRow, NpcRow,
        SoftDeleteRow,
    };
}

pub use db::Database;

pub trait VaultRepository: Send + Sync {
    #[allow(dead_code)]
    fn read_file(&self, vault: &Vault, path: &str) -> Result<Option<String>, String>;
    #[allow(dead_code)]
    fn write_file(&self, vault: &Vault, path: &str, contents: &str) -> Result<(), String>;
    #[allow(dead_code)]
    fn move_file(&self, vault: &Vault, from: &str, to: &str) -> Result<(), String>;
    #[allow(dead_code)]
    fn file_exists(&self, vault: &Vault, path: &str) -> Result<bool, String>;
    #[allow(dead_code)]
    fn resolve_path(&self, vault: &Vault, path: &str) -> Result<PathBuf, String>;
    #[allow(dead_code)]
    fn ensure_root_exists(&self, vault: &Vault) -> Result<(), String>;
    fn ensure_structure(&self, vault: &Vault) -> Result<(), String>;
}

pub struct ProdVaultRepository;

impl VaultRepository for ProdVaultRepository {
    fn read_file(&self, vault: &Vault, path: &str) -> Result<Option<String>, String> {
        let relative = PathBuf::from(normalize_relative_path(path));
        let full = vault
            .resolve_relative(&relative)
            .map_err(|e| e.to_string())?;
        if !full.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(&full)
            .map(Some)
            .map_err(|e| format!("failed to read vault file {}: {}", full.display(), e))
    }

    fn write_file(&self, vault: &Vault, path: &str, contents: &str) -> Result<(), String> {
        vault
            .write_relative(&PathBuf::from(path), contents)
            .map_err(|e| e.to_string())
    }

    fn move_file(&self, vault: &Vault, from: &str, to: &str) -> Result<(), String> {
        let from_normalized = normalize_relative_path(from);
        let to_normalized = normalize_relative_path(to);
        let from_full = vault
            .resolve_relative(&PathBuf::from(&from_normalized))
            .map_err(|e| e.to_string())?;
        if !from_full.exists() {
            return Err(format!(
                "source file does not exist: {}",
                from_full.display()
            ));
        }
        let to_full = vault
            .resolve_relative(&PathBuf::from(&to_normalized))
            .map_err(|e| e.to_string())?;
        if let Some(parent) = to_full.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create directory {}: {}", parent.display(), e))?;
        }
        std::fs::rename(&from_full, &to_full).map_err(|e| {
            format!(
                "failed to move file from {} to {}: {}",
                from_full.display(),
                to_full.display(),
                e
            )
        })
    }

    fn file_exists(&self, vault: &Vault, path: &str) -> Result<bool, String> {
        let full = vault
            .resolve_relative(&PathBuf::from(path))
            .map_err(|e| e.to_string())?;
        Ok(full.exists())
    }

    fn resolve_path(&self, vault: &Vault, path: &str) -> Result<PathBuf, String> {
        vault
            .resolve_relative(&PathBuf::from(path))
            .map_err(|e| e.to_string())
    }

    fn ensure_root_exists(&self, vault: &Vault) -> Result<(), String> {
        vault.ensure_root_exists().map_err(|e| e.to_string())
    }

    fn ensure_structure(&self, vault: &Vault) -> Result<(), String> {
        vault.ensure_structure().map_err(|e| e.to_string())
    }
}

#[allow(dead_code)]
fn normalize_relative_path(path: &str) -> String {
    path.replace('\\', "/")
}

#[async_trait]
pub trait NpcRepository: Send + Sync {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::NpcRow>, String>;
    async fn find_by_id(&self, database: &Database, id: &str)
    -> Result<Option<db::NpcRow>, String>;
    async fn upsert(&self, database: &Database, row: &db::NpcRow) -> Result<(), String>;
    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::NpcRow>, String>;
    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String>;
    async fn list_all(&self, database: &Database) -> Result<Vec<db::NpcRow>, String>;
}

pub struct ProdNpcRepository;

#[async_trait]
impl NpcRepository for ProdNpcRepository {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::NpcRow>, String> {
        core_db::find_npc_by_name_or_slug(&database.pool, name_or_slug)
            .await
            .map_err(|e| e.to_string())
    }

    async fn find_by_id(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<Option<db::NpcRow>, String> {
        core_db::find_npc_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn upsert(&self, database: &Database, row: &db::NpcRow) -> Result<(), String> {
        core_db::upsert_npc(&database.pool, row)
            .await
            .map_err(|e| e.to_string())
    }

    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::NpcRow>, String> {
        core_db::search_npcs_by_name(&database.pool, query, limit)
            .await
            .map_err(|e| e.to_string())
    }

    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String> {
        core_db::delete_npc_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn list_all(&self, database: &Database) -> Result<Vec<db::NpcRow>, String> {
        core_db::list_npcs(&database.pool)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
pub trait LocationRepository: Send + Sync {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::LocationRow>, String>;
    async fn find_by_id(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<Option<db::LocationRow>, String>;
    async fn find_by_slug(
        &self,
        database: &Database,
        slug: &str,
    ) -> Result<Option<db::LocationRow>, String>;
    async fn upsert(&self, database: &Database, row: &db::LocationRow) -> Result<(), String>;
    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::LocationRow>, String>;
    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String>;
    async fn list_all(&self, database: &Database) -> Result<Vec<db::LocationRow>, String>;
}

pub struct ProdLocationRepository;

#[async_trait]
impl LocationRepository for ProdLocationRepository {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::LocationRow>, String> {
        core_db::find_location_by_name_or_slug(&database.pool, name_or_slug)
            .await
            .map_err(|e| e.to_string())
    }

    async fn find_by_id(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<Option<db::LocationRow>, String> {
        core_db::find_location_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn find_by_slug(
        &self,
        database: &Database,
        slug: &str,
    ) -> Result<Option<db::LocationRow>, String> {
        core_db::find_location_by_slug(&database.pool, slug)
            .await
            .map_err(|e| e.to_string())
    }

    async fn upsert(&self, database: &Database, row: &db::LocationRow) -> Result<(), String> {
        core_db::upsert_location(&database.pool, row)
            .await
            .map_err(|e| e.to_string())
    }

    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::LocationRow>, String> {
        core_db::search_locations_by_name(&database.pool, query, limit)
            .await
            .map_err(|e| e.to_string())
    }

    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String> {
        core_db::delete_location_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn list_all(&self, database: &Database) -> Result<Vec<db::LocationRow>, String> {
        core_db::list_locations(&database.pool)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
pub trait FactionRepository: Send + Sync {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::FactionRow>, String>;
    async fn find_by_id(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<Option<db::FactionRow>, String>;
    async fn upsert(&self, database: &Database, row: &db::FactionRow) -> Result<(), String>;
    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::FactionRow>, String>;
    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String>;
    async fn list_all(&self, database: &Database) -> Result<Vec<db::FactionRow>, String>;
}

pub struct ProdFactionRepository;

#[async_trait]
impl FactionRepository for ProdFactionRepository {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::FactionRow>, String> {
        core_db::find_faction_by_name_or_slug(&database.pool, name_or_slug)
            .await
            .map_err(|e| e.to_string())
    }

    async fn find_by_id(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<Option<db::FactionRow>, String> {
        core_db::find_faction_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn upsert(&self, database: &Database, row: &db::FactionRow) -> Result<(), String> {
        core_db::upsert_faction(&database.pool, row)
            .await
            .map_err(|e| e.to_string())
    }

    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::FactionRow>, String> {
        core_db::search_factions_by_name(&database.pool, query, limit)
            .await
            .map_err(|e| e.to_string())
    }

    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String> {
        core_db::delete_faction_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn list_all(&self, database: &Database) -> Result<Vec<db::FactionRow>, String> {
        core_db::list_factions(&database.pool)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
pub trait ItemRepository: Send + Sync {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::ItemRow>, String>;
    async fn find_by_id(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<Option<db::ItemRow>, String>;
    async fn upsert(&self, database: &Database, row: &db::ItemRow) -> Result<(), String>;
    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::ItemRow>, String>;
    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String>;
    async fn list_all(&self, database: &Database) -> Result<Vec<db::ItemRow>, String>;
}

pub struct ProdItemRepository;

#[async_trait]
impl ItemRepository for ProdItemRepository {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::ItemRow>, String> {
        core_db::find_item_by_name_or_slug(&database.pool, name_or_slug)
            .await
            .map_err(|e| e.to_string())
    }

    async fn find_by_id(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<Option<db::ItemRow>, String> {
        core_db::find_item_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn upsert(&self, database: &Database, row: &db::ItemRow) -> Result<(), String> {
        core_db::upsert_item(&database.pool, row)
            .await
            .map_err(|e| e.to_string())
    }

    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::ItemRow>, String> {
        core_db::search_items_by_name(&database.pool, query, limit)
            .await
            .map_err(|e| e.to_string())
    }

    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String> {
        core_db::delete_item_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn list_all(&self, database: &Database) -> Result<Vec<db::ItemRow>, String> {
        core_db::list_items(&database.pool)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::EventRow>, String>;
    async fn find_by_id(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<Option<db::EventRow>, String>;
    async fn upsert(&self, database: &Database, row: &db::EventRow) -> Result<(), String>;
    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::EventRow>, String>;
    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String>;
    async fn list_all(&self, database: &Database) -> Result<Vec<db::EventRow>, String>;
}

pub struct ProdEventRepository;

#[async_trait]
impl EventRepository for ProdEventRepository {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::EventRow>, String> {
        core_db::find_event_by_name_or_slug(&database.pool, name_or_slug)
            .await
            .map_err(|e| e.to_string())
    }

    async fn find_by_id(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<Option<db::EventRow>, String> {
        core_db::find_event_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn upsert(&self, database: &Database, row: &db::EventRow) -> Result<(), String> {
        core_db::upsert_event(&database.pool, row)
            .await
            .map_err(|e| e.to_string())
    }

    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::EventRow>, String> {
        core_db::search_events_by_name(&database.pool, query, limit)
            .await
            .map_err(|e| e.to_string())
    }

    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String> {
        core_db::delete_event_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn list_all(&self, database: &Database) -> Result<Vec<db::EventRow>, String> {
        core_db::list_events(&database.pool)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
pub trait GodRepository: Send + Sync {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::GodRow>, String>;
    async fn find_by_id(&self, database: &Database, id: &str)
    -> Result<Option<db::GodRow>, String>;
    async fn upsert(&self, database: &Database, row: &db::GodRow) -> Result<(), String>;
    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::GodRow>, String>;
    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String>;
    async fn list_all(&self, database: &Database) -> Result<Vec<db::GodRow>, String>;
}

pub struct ProdGodRepository;

#[async_trait]
impl GodRepository for ProdGodRepository {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::GodRow>, String> {
        core_db::find_god_by_name_or_slug(&database.pool, name_or_slug)
            .await
            .map_err(|e| e.to_string())
    }

    async fn find_by_id(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<Option<db::GodRow>, String> {
        core_db::find_god_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn upsert(&self, database: &Database, row: &db::GodRow) -> Result<(), String> {
        core_db::upsert_god(&database.pool, row)
            .await
            .map_err(|e| e.to_string())
    }

    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::GodRow>, String> {
        core_db::search_gods_by_name(&database.pool, query, limit)
            .await
            .map_err(|e| e.to_string())
    }

    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String> {
        core_db::delete_god_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn list_all(&self, database: &Database) -> Result<Vec<db::GodRow>, String> {
        core_db::list_gods(&database.pool)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
pub trait DungeonRepository: Send + Sync {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::DungeonRow>, String>;
    async fn find_by_id(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<Option<db::DungeonRow>, String>;
    async fn upsert(&self, database: &Database, row: &db::DungeonRow) -> Result<(), String>;
    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::DungeonRow>, String>;
    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String>;
    async fn list_all(&self, database: &Database) -> Result<Vec<db::DungeonRow>, String>;
}

pub struct ProdDungeonRepository;

#[async_trait]
impl DungeonRepository for ProdDungeonRepository {
    async fn find_by_name_or_slug(
        &self,
        database: &Database,
        name_or_slug: &str,
    ) -> Result<Option<db::DungeonRow>, String> {
        core_db::find_dungeon_by_name_or_slug(&database.pool, name_or_slug)
            .await
            .map_err(|e| e.to_string())
    }

    async fn find_by_id(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<Option<db::DungeonRow>, String> {
        core_db::find_dungeon_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn upsert(&self, database: &Database, row: &db::DungeonRow) -> Result<(), String> {
        core_db::upsert_dungeon(&database.pool, row)
            .await
            .map_err(|e| e.to_string())
    }

    async fn search_by_name(
        &self,
        database: &Database,
        query: &str,
        limit: i64,
    ) -> Result<Vec<db::DungeonRow>, String> {
        core_db::search_dungeons_by_name(&database.pool, query, limit)
            .await
            .map_err(|e| e.to_string())
    }

    async fn delete_by_id(&self, database: &Database, id: &str) -> Result<(), String> {
        core_db::delete_dungeon_by_id(&database.pool, id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn list_all(&self, database: &Database) -> Result<Vec<db::DungeonRow>, String> {
        core_db::list_dungeons(&database.pool)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
pub trait DocumentRepository: Send + Sync {
    async fn find_by_vault_path(
        &self,
        database: &Database,
        vault_path: &str,
    ) -> Result<Option<String>, String>;
    // P6 (cleanup-0.5.0): the 8-arg document-index upsert is bundled into a
    // struct when P6 reworks the repository layer (transactions + index upsert).
    // Remove this allow then.
    #[allow(clippy::too_many_arguments)]
    async fn upsert_index(
        &self,
        database: &Database,
        entity_type: &str,
        slug: &str,
        name: Option<&str>,
        vault_path: &str,
        created_at: &str,
        updated_at: &str,
    ) -> Result<(), String>;
    async fn delete_by_vault_path(
        &self,
        database: &Database,
        vault_path: &str,
    ) -> Result<(), String>;
}

pub struct ProdDocumentRepository;

#[async_trait]
impl DocumentRepository for ProdDocumentRepository {
    async fn find_by_vault_path(
        &self,
        database: &Database,
        vault_path: &str,
    ) -> Result<Option<String>, String> {
        core_db::find_document_by_vault_path(&database.pool, vault_path)
            .await
            .map_err(|e| e.to_string())
    }

    async fn upsert_index(
        &self,
        database: &Database,
        entity_type: &str,
        slug: &str,
        name: Option<&str>,
        vault_path: &str,
        created_at: &str,
        updated_at: &str,
    ) -> Result<(), String> {
        core_db::upsert_document_index(
            &database.pool,
            entity_type,
            slug,
            name,
            vault_path,
            created_at,
            updated_at,
        )
        .await
        .map_err(|e| e.to_string())
    }

    async fn delete_by_vault_path(
        &self,
        database: &Database,
        vault_path: &str,
    ) -> Result<(), String> {
        core_db::delete_document_by_vault_path(&database.pool, vault_path)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
pub trait GenerationRepository: Send + Sync {
    async fn insert(
        &self,
        database: &Database,
        gen_type: &str,
        input: Option<&str>,
        output: &str,
    ) -> Result<(), String>;
    async fn recent_prompts(
        &self,
        database: &Database,
        gen_type: &str,
        limit: i64,
    ) -> Result<Vec<String>, String>;
}

pub struct ProdGenerationRepository;

#[async_trait]
impl GenerationRepository for ProdGenerationRepository {
    async fn insert(
        &self,
        database: &Database,
        gen_type: &str,
        input: Option<&str>,
        output: &str,
    ) -> Result<(), String> {
        core_db::insert_generation(&database.pool, gen_type, input, output)
            .await
            .map_err(|e| e.to_string())
    }

    async fn recent_prompts(
        &self,
        database: &Database,
        gen_type: &str,
        limit: i64,
    ) -> Result<Vec<String>, String> {
        core_db::recent_generation_prompts(&database.pool, gen_type, limit)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
pub trait SoftDeleteRepository: Send + Sync {
    async fn insert(&self, database: &Database, row: &db::SoftDeleteRow) -> Result<(), String>;
    async fn latest_pending(
        &self,
        database: &Database,
    ) -> Result<Option<db::SoftDeleteRow>, String>;
    async fn mark_undone(
        &self,
        database: &Database,
        id: i64,
        timestamp: &str,
    ) -> Result<(), String>;
    async fn finalize_pending_publishes(
        &self,
        database: &Database,
        timestamp: &str,
    ) -> Result<(), String>;
}

pub struct ProdSoftDeleteRepository;

#[async_trait]
impl SoftDeleteRepository for ProdSoftDeleteRepository {
    async fn insert(&self, database: &Database, row: &db::SoftDeleteRow) -> Result<(), String> {
        core_db::insert_soft_delete(&database.pool, row)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn latest_pending(
        &self,
        database: &Database,
    ) -> Result<Option<db::SoftDeleteRow>, String> {
        core_db::latest_pending_soft_delete(&database.pool)
            .await
            .map_err(|e| e.to_string())
    }

    async fn mark_undone(
        &self,
        database: &Database,
        id: i64,
        timestamp: &str,
    ) -> Result<(), String> {
        core_db::mark_soft_delete_undone(&database.pool, id, timestamp)
            .await
            .map_err(|e| e.to_string())
    }

    async fn finalize_pending_publishes(
        &self,
        database: &Database,
        timestamp: &str,
    ) -> Result<(), String> {
        core_db::finalize_pending_publishes(&database.pool, timestamp)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
