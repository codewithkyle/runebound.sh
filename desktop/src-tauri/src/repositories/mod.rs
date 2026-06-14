use dnd_core::vault::Vault;
use std::path::PathBuf;

pub mod db {
    pub use dnd_core::db::{FactionRow, LocationRow, NpcRow, SoftDeleteRow};
}

pub trait VaultRepository: Send + Sync {
    fn read_file(&self, vault: &Vault, path: &str) -> Result<Option<String>, String>;
    fn write_file(&self, vault: &Vault, path: &str, contents: &str) -> Result<(), String>;
    fn move_file(&self, vault: &Vault, from: &str, to: &str) -> Result<(), String>;
    fn file_exists(&self, vault: &Vault, path: &str) -> Result<bool, String>;
    fn resolve_path(&self, vault: &Vault, path: &str) -> Result<PathBuf, String>;
    fn ensure_root_exists(&self, vault: &Vault) -> Result<(), String>;
    fn ensure_structure(&self, vault: &Vault) -> Result<(), String>;
}

pub struct ProdVaultRepository;

impl VaultRepository for ProdVaultRepository {
    fn read_file(&self, vault: &Vault, path: &str) -> Result<Option<String>, String> {
        let relative = PathBuf::from(normalize_relative_path(path));
        let full = vault.resolve_relative(&relative).map_err(|e| e.to_string())?;
        if !full.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(&full)
            .map(Some)
            .map_err(|e| format!("failed to read vault file {}: {}", full.display(), e))
    }

    fn write_file(&self, vault: &Vault, path: &str, contents: &str) -> Result<(), String> {
        vault.write_relative(&PathBuf::from(path), contents).map_err(|e| e.to_string())
    }

    fn move_file(&self, vault: &Vault, from: &str, to: &str) -> Result<(), String> {
        let from_normalized = normalize_relative_path(from);
        let to_normalized = normalize_relative_path(to);
        let from_full = vault.resolve_relative(&PathBuf::from(&from_normalized)).map_err(|e| e.to_string())?;
        if !from_full.exists() {
            return Err(format!("source file does not exist: {}", from_full.display()));
        }
        let to_full = vault.resolve_relative(&PathBuf::from(&to_normalized)).map_err(|e| e.to_string())?;
        if let Some(parent) = to_full.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("failed to create directory {}: {}", parent.display(), e))?;
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
        let full = vault.resolve_relative(&PathBuf::from(path)).map_err(|e| e.to_string())?;
        Ok(full.exists())
    }

    fn resolve_path(&self, vault: &Vault, path: &str) -> Result<PathBuf, String> {
        vault.resolve_relative(&PathBuf::from(path)).map_err(|e| e.to_string())
    }

    fn ensure_root_exists(&self, vault: &Vault) -> Result<(), String> {
        vault.ensure_root_exists().map_err(|e| e.to_string())
    }

    fn ensure_structure(&self, vault: &Vault) -> Result<(), String> {
        vault.ensure_structure().map_err(|e| e.to_string())
    }
}

fn normalize_relative_path(path: &str) -> String {
    path.replace('\\', "/")
}

// Database repository traits - commented out until SqlitePool type is accessible
// pub trait NpcRepository: Send + Sync {
//     async fn find_by_name_or_slug(&self, pool: &SqlitePool, name_or_slug: &str) -> Result<Option<NpcRow>, String>;
//     async fn find_by_id(&self, pool: &SqlitePool, id: &str) -> Result<Option<NpcRow>, String>;
//     async fn upsert(&self, pool: &SqlitePool, row: &NpcRow) -> Result<(), String>;
//     async fn search_by_name(&self, pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<NpcRow>, String>;
//     async fn delete_by_id(&self, pool: &SqlitePool, id: &str) -> Result<(), String>;
// }

// pub struct ProdNpcRepository;

// impl NpcRepository for ProdNpcRepository {
//     async fn find_by_name_or_slug(&self, pool: &SqlitePool, name_or_slug: &str) -> Result<Option<NpcRow>, String> {
//         dnd_core::db::find_npc_by_name_or_slug(pool, name_or_slug).await.map_err(|e| e.to_string())
//     }
//     async fn find_by_id(&self, pool: &SqlitePool, id: &str) -> Result<Option<NpcRow>, String> {
//         dnd_core::db::find_npc_by_id(pool, id).await.map_err(|e| e.to_string())
//     }
//     async fn upsert(&self, pool: &SqlitePool, row: &NpcRow) -> Result<(), String> {
//         dnd_core::db::upsert_npc(pool, row).await.map_err(|e| e.to_string())
//     }
//     async fn search_by_name(&self, pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<NpcRow>, String> {
//         dnd_core::db::search_npcs_by_name(pool, query, limit).await.map_err(|e| e.to_string())
//     }
//     async fn delete_by_id(&self, pool: &SqlitePool, id: &str) -> Result<(), String> {
//         dnd_core::db::delete_npc_by_id(pool, id).await.map_err(|e| e.to_string())
//     }
// }

// pub trait LocationRepository: Send + Sync {
//     async fn find_by_name_or_slug(&self, pool: &SqlitePool, name_or_slug: &str) -> Result<Option<LocationRow>, String>;
//     async fn find_by_id(&self, pool: &SqlitePool, id: &str) -> Result<Option<LocationRow>, String>;
//     async fn find_by_slug(&self, pool: &SqlitePool, slug: &str) -> Result<Option<LocationRow>, String>;
//     async fn upsert(&self, pool: &SqlitePool, row: &LocationRow) -> Result<(), String>;
//     async fn search_by_name(&self, pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<LocationRow>, String>;
//     async fn delete_by_id(&self, pool: &SqlitePool, id: &str) -> Result<(), String>;
// }

// pub struct ProdLocationRepository;

// impl LocationRepository for ProdLocationRepository {
//     async fn find_by_name_or_slug(&self, pool: &SqlitePool, name_or_slug: &str) -> Result<Option<LocationRow>, String> {
//         dnd_core::db::find_location_by_name_or_slug(pool, name_or_slug).await.map_err(|e| e.to_string())
//     }
//     async fn find_by_id(&self, pool: &SqlitePool, id: &str) -> Result<Option<LocationRow>, String> {
//         dnd_core::db::find_location_by_id(pool, id).await.map_err(|e| e.to_string())
//     }
//     async fn find_by_slug(&self, pool: &SqlitePool, slug: &str) -> Result<Option<LocationRow>, String> {
//         dnd_core::db::find_location_by_slug(pool, slug).await.map_err(|e| e.to_string())
//     }
//     async fn upsert(&self, pool: &SqlitePool, row: &LocationRow) -> Result<(), String> {
//         dnd_core::db::upsert_location(pool, row).await.map_err(|e| e.to_string())
//     }
//     async fn search_by_name(&self, pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<LocationRow>, String> {
//         dnd_core::db::search_locations_by_name(pool, query, limit).await.map_err(|e| e.to_string())
//     }
//     async fn delete_by_id(&self, pool: &SqlitePool, id: &str) -> Result<(), String> {
//         dnd_core::db::delete_location_by_id(pool, id).await.map_err(|e| e.to_string())
//     }
// }

// pub trait FactionRepository: Send + Sync {
//     async fn find_by_name_or_slug(&self, pool: &SqlitePool, name_or_slug: &str) -> Result<Option<FactionRow>, String>;
//     async fn find_by_id(&self, pool: &SqlitePool, id: &str) -> Result<Option<FactionRow>, String>;
//     async fn upsert(&self, pool: &SqlitePool, row: &FactionRow) -> Result<(), String>;
//     async fn search_by_name(&self, pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<FactionRow>, String>;
//     async fn delete_by_id(&self, pool: &SqlitePool, id: &str) -> Result<(), String>;
// }

// pub struct ProdFactionRepository;

// impl FactionRepository for ProdFactionRepository {
//     async fn find_by_name_or_slug(&self, pool: &SqlitePool, name_or_slug: &str) -> Result<Option<FactionRow>, String> {
//         dnd_core::db::find_faction_by_name_or_slug(pool, name_or_slug).await.map_err(|e| e.to_string())
//     }
//     async fn find_by_id(&self, pool: &SqlitePool, id: &str) -> Result<Option<FactionRow>, String> {
//         dnd_core::db::find_faction_by_id(pool, id).await.map_err(|e| e.to_string())
//     }
//     async fn upsert(&self, pool: &SqlitePool, row: &FactionRow) -> Result<(), String> {
//         dnd_core::db::upsert_faction(pool, row).await.map_err(|e| e.to_string())
//     }
//     async fn search_by_name(&self, pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<FactionRow>, String> {
//         dnd_core::db::search_factions_by_name(pool, query, limit).await.map_err(|e| e.to_string())
//     }
//     async fn delete_by_id(&self, pool: &SqlitePool, id: &str) -> Result<(), String> {
//         dnd_core::db::delete_faction_by_id(pool, id).await.map_err(|e| e.to_string())
//     }
// }

// pub trait DocumentRepository: Send + Sync {
//     async fn upsert_index(&self, pool: &SqlitePool, entity_type: &str, slug: &str, name: Option<&str>, vault_path: &str, created_at: &str, updated_at: &str) -> Result<(), String>;
//     async fn delete_by_vault_path(&self, pool: &SqlitePool, vault_path: &str) -> Result<(), String>;
// }

// pub struct ProdDocumentRepository;

// impl DocumentRepository for ProdDocumentRepository {
//     async fn upsert_index(&self, pool: &SqlitePool, entity_type: &str, slug: &str, name: Option<&str>, vault_path: &str, created_at: &str, updated_at: &str) -> Result<(), String> {
//         dnd_core::db::upsert_document_index(pool, entity_type, slug, name, vault_path, created_at, updated_at).await.map_err(|e| e.to_string())
//     }
//     async fn delete_by_vault_path(&self, pool: &SqlitePool, vault_path: &str) -> Result<(), String> {
//         dnd_core::db::delete_document_by_vault_path(pool, vault_path).await.map_err(|e| e.to_string())
//     }
// }

// pub trait GenerationRepository: Send + Sync {
//     async fn insert(&self, pool: &SqlitePool, gen_type: &str, input: Option<&str>, output: &str) -> Result<(), String>;
//     async fn recent_prompts(&self, pool: &SqlitePool, gen_type: &str, limit: i64) -> Result<Vec<String>, String>;
// }

// pub struct ProdGenerationRepository;

// impl GenerationRepository for ProdGenerationRepository {
//     async fn insert(&self, pool: &SqlitePool, gen_type: &str, input: Option<&str>, output: &str) -> Result<(), String> {
//         dnd_core::db::insert_generation(pool, gen_type, input, output).await.map_err(|e| e.to_string())
//     }
//     async fn recent_prompts(&self, pool: &SqlitePool, gen_type: &str, limit: i64) -> Result<Vec<String>, String> {
//         dnd_core::db::recent_generation_prompts(pool, gen_type, limit).await.map_err(|e| e.to_string())
//     }
// }

// pub trait SoftDeleteRepository: Send + Sync {
//     async fn insert(&self, pool: &SqlitePool, row: &SoftDeleteRow) -> Result<(), String>;
//     async fn latest_pending(&self, pool: &SqlitePool) -> Result<Option<SoftDeleteRow>, String>;
//     async fn mark_undone(&self, pool: &SqlitePool, id: i64, timestamp: &str) -> Result<(), String>;
// }

// pub struct ProdSoftDeleteRepository;

// impl SoftDeleteRepository for ProdSoftDeleteRepository {
//     async fn insert(&self, pool: &SqlitePool, row: &SoftDeleteRow) -> Result<(), String> {
//         dnd_core::db::insert_soft_delete(pool, row).await.map_err(|e| e.to_string())
//     }
//     async fn latest_pending(&self, pool: &SqlitePool) -> Result<Option<SoftDeleteRow>, String> {
//         dnd_core::db::latest_pending_soft_delete(pool).await.map_err(|e| e.to_string())
//     }
//     async fn mark_undone(&self, pool: &SqlitePool, id: i64, timestamp: &str) -> Result<(), String> {
//         dnd_core::db::mark_soft_delete_undone(pool, id, timestamp).await.map_err(|e| e.to_string())
//     }
// }