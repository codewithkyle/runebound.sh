use std::collections::HashSet;
use std::path::PathBuf;

use async_trait::async_trait;
use dnd_core::config::load_effective;
use dnd_core::entity_store::EntityStore;
use dnd_core::npc::{
    DungeonFrontmatter, EventFrontmatter, FactionFrontmatter, GodFrontmatter, ItemFrontmatter,
    LocationFrontmatter, NpcFrontmatter, normalize_markdown_file_stem, now_timestamp,
};
use dnd_core::serialization::{carrying_to_db_text, exports_to_db_text, faction_list_to_db_text};
use dnd_core::vault::Vault;

use crate::app_state::AppState;
use crate::repositories::{
    DocumentRepository, DungeonRepository, EventRepository, FactionRepository, GodRepository,
    ItemRepository, LocationRepository, NpcRepository, db,
};
use crate::utils::normalize_relative_path_for_storage;

pub struct VaultSyncService;

impl VaultSyncService {
    pub async fn sync_from_vault(&self, state: &AppState) -> Result<(), String> {
        let loaded = load_effective().map_err(|err| err.to_string())?;
        if !loaded.effective.vault.autoscan_on_start {
            return Ok(());
        }

        let Some(vault_path) = loaded.effective.vault.path.clone() else {
            return Ok(());
        };

        let vault = Vault::new(vault_path);
        vault.ensure_structure().map_err(|err| err.to_string())?;

        let store = EntityStore::new().map_err(|err| err.to_string())?;
        let database = state.database();
        let npc_repo = state.npc_repo();
        let location_repo = state.location_repo();
        let faction_repo = state.faction_repo();
        let item_repo = state.item_repo();
        let event_repo = state.event_repo();
        let god_repo = state.god_repo();
        let dungeon_repo = state.dungeon_repo();
        let document_repo = state.document_repo();

        // Any publish that survived to a restart is permanent: finalize its pending
        // undo record so a reaped entity can no longer be brought back via `undo`.
        state
            .soft_delete_repo()
            .finalize_pending_publishes(database.as_ref(), &now_timestamp())
            .await?;

        let database = database.as_ref();
        let document_repo = document_repo.as_ref();
        sync_entities(&NpcSync(npc_repo.as_ref()), &store, database, document_repo).await?;
        sync_entities(
            &LocationSync(location_repo.as_ref()),
            &store,
            database,
            document_repo,
        )
        .await?;
        sync_entities(
            &FactionSync(faction_repo.as_ref()),
            &store,
            database,
            document_repo,
        )
        .await?;
        sync_entities(
            &ItemSync(item_repo.as_ref()),
            &store,
            database,
            document_repo,
        )
        .await?;
        sync_entities(
            &EventSync(event_repo.as_ref()),
            &store,
            database,
            document_repo,
        )
        .await?;
        sync_entities(&GodSync(god_repo.as_ref()), &store, database, document_repo).await?;
        sync_entities(
            &DungeonSync(dungeon_repo.as_ref()),
            &store,
            database,
            document_repo,
        )
        .await?;

        Ok(())
    }
}

/// Frontmatter fields the generic reconcile loop reads, normalized across kinds.
struct StoreView<'a> {
    id: &'a str,
    slug: &'a str,
    vault_path: &'a str,
    published: bool,
}

/// DB-row fields needed for the document index and stale-prune, normalized across kinds.
struct RowView<'a> {
    id: &'a str,
    slug: &'a str,
    name: &'a str,
    vault_path: &'a str,
    created_at: &'a str,
    updated_at: &'a str,
}

/// Per-kind glue for [`sync_entities`]. Each impl wraps the kind's repository and
/// exposes its typed store/DB operations through one uniform interface so the
/// reconcile logic lives in exactly one place.
#[async_trait]
trait SyncRepository: Send + Sync {
    type Frontmatter: Send;
    type Row: Send + Sync;

    /// Document-index kind string (e.g. `"npc"`).
    const KIND: &'static str;

    /// Canonical TOML records for this kind.
    fn list_store(&self, store: &EntityStore) -> Result<Vec<Self::Frontmatter>, String>;

    /// Delete the backing TOML for a reaped (published) entity.
    fn delete_from_store(&self, store: &EntityStore, slug: &str) -> Result<(), String>;

    /// Build the DB row from a frontmatter record.
    fn row_from_frontmatter(frontmatter: &Self::Frontmatter) -> Result<Self::Row, String>;

    fn frontmatter_view(frontmatter: &Self::Frontmatter) -> StoreView<'_>;
    fn row_view(row: &Self::Row) -> RowView<'_>;

    async fn upsert(&self, database: &db::Database, row: &Self::Row) -> Result<(), String>;
    async fn list_all(&self, database: &db::Database) -> Result<Vec<Self::Row>, String>;
    async fn delete_by_id(&self, database: &db::Database, id: &str) -> Result<(), String>;
}

/// Reconcile one entity kind's canonical TOML store into the database + document
/// index: reap published records, upsert the rest, then prune DB rows whose TOML
/// no longer exists.
async fn sync_entities<K: SyncRepository>(
    plan: &K,
    store: &EntityStore,
    database: &db::Database,
    document_repo: &dyn DocumentRepository,
) -> Result<(), String> {
    let frontmatters = plan.list_store(store)?;
    let mut synced_ids = HashSet::new();

    for frontmatter in &frontmatters {
        let view = K::frontmatter_view(frontmatter);
        // Published entities are reaped: their record lives in Obsidian now, so drop
        // the DB row, document index, and the backing TOML.
        if view.published {
            plan.delete_by_id(database, view.id).await?;
            document_repo
                .delete_by_vault_path(database, view.vault_path)
                .await?;
            plan.delete_from_store(store, view.slug)?;
            continue;
        }

        let row = K::row_from_frontmatter(frontmatter)?;
        let row_view = K::row_view(&row);
        synced_ids.insert(row_view.id.to_string());
        plan.upsert(database, &row).await?;
        document_repo
            .upsert_index(
                database,
                K::KIND,
                row_view.slug,
                Some(row_view.name),
                row_view.vault_path,
                row_view.created_at,
                row_view.updated_at,
            )
            .await?;
    }

    let existing = plan.list_all(database).await?;
    for row in &existing {
        let row_view = K::row_view(row);
        if !synced_ids.contains(row_view.id) {
            plan.delete_by_id(database, row_view.id).await?;
            document_repo
                .delete_by_vault_path(database, row_view.vault_path)
                .await?;
        }
    }

    Ok(())
}

struct NpcSync<'a>(&'a dyn NpcRepository);

#[async_trait]
impl SyncRepository for NpcSync<'_> {
    type Frontmatter = NpcFrontmatter;
    type Row = db::NpcRow;
    const KIND: &'static str = "npc";

    fn list_store(&self, store: &EntityStore) -> Result<Vec<Self::Frontmatter>, String> {
        store.list_npcs().map_err(|err| err.to_string())
    }
    fn delete_from_store(&self, store: &EntityStore, slug: &str) -> Result<(), String> {
        store.delete_npc(slug).map_err(|err| err.to_string())
    }
    fn row_from_frontmatter(frontmatter: &Self::Frontmatter) -> Result<Self::Row, String> {
        npc_row_from_frontmatter(frontmatter)
    }
    fn frontmatter_view(frontmatter: &Self::Frontmatter) -> StoreView<'_> {
        StoreView {
            id: &frontmatter.id,
            slug: &frontmatter.slug,
            vault_path: &frontmatter.vault_path,
            published: frontmatter.published_at.is_some(),
        }
    }
    fn row_view(row: &Self::Row) -> RowView<'_> {
        RowView {
            id: &row.id,
            slug: &row.slug,
            name: &row.name,
            vault_path: &row.vault_path,
            created_at: &row.created_at,
            updated_at: &row.updated_at,
        }
    }
    async fn upsert(&self, database: &db::Database, row: &Self::Row) -> Result<(), String> {
        self.0.upsert(database, row).await
    }
    async fn list_all(&self, database: &db::Database) -> Result<Vec<Self::Row>, String> {
        self.0.list_all(database).await
    }
    async fn delete_by_id(&self, database: &db::Database, id: &str) -> Result<(), String> {
        self.0.delete_by_id(database, id).await
    }
}

struct LocationSync<'a>(&'a dyn LocationRepository);

#[async_trait]
impl SyncRepository for LocationSync<'_> {
    type Frontmatter = LocationFrontmatter;
    type Row = db::LocationRow;
    const KIND: &'static str = "location";

    fn list_store(&self, store: &EntityStore) -> Result<Vec<Self::Frontmatter>, String> {
        store.list_locations().map_err(|err| err.to_string())
    }
    fn delete_from_store(&self, store: &EntityStore, slug: &str) -> Result<(), String> {
        store.delete_location(slug).map_err(|err| err.to_string())
    }
    fn row_from_frontmatter(frontmatter: &Self::Frontmatter) -> Result<Self::Row, String> {
        location_row_from_frontmatter(frontmatter)
    }
    fn frontmatter_view(frontmatter: &Self::Frontmatter) -> StoreView<'_> {
        StoreView {
            id: &frontmatter.id,
            slug: &frontmatter.slug,
            vault_path: &frontmatter.vault_path,
            published: frontmatter.published_at.is_some(),
        }
    }
    fn row_view(row: &Self::Row) -> RowView<'_> {
        RowView {
            id: &row.id,
            slug: &row.slug,
            name: &row.name,
            vault_path: &row.vault_path,
            created_at: &row.created_at,
            updated_at: &row.updated_at,
        }
    }
    async fn upsert(&self, database: &db::Database, row: &Self::Row) -> Result<(), String> {
        self.0.upsert(database, row).await
    }
    async fn list_all(&self, database: &db::Database) -> Result<Vec<Self::Row>, String> {
        self.0.list_all(database).await
    }
    async fn delete_by_id(&self, database: &db::Database, id: &str) -> Result<(), String> {
        self.0.delete_by_id(database, id).await
    }
}

struct FactionSync<'a>(&'a dyn FactionRepository);

#[async_trait]
impl SyncRepository for FactionSync<'_> {
    type Frontmatter = FactionFrontmatter;
    type Row = db::FactionRow;
    const KIND: &'static str = "faction";

    fn list_store(&self, store: &EntityStore) -> Result<Vec<Self::Frontmatter>, String> {
        store.list_factions().map_err(|err| err.to_string())
    }
    fn delete_from_store(&self, store: &EntityStore, slug: &str) -> Result<(), String> {
        store.delete_faction(slug).map_err(|err| err.to_string())
    }
    fn row_from_frontmatter(frontmatter: &Self::Frontmatter) -> Result<Self::Row, String> {
        faction_row_from_frontmatter(frontmatter)
    }
    fn frontmatter_view(frontmatter: &Self::Frontmatter) -> StoreView<'_> {
        StoreView {
            id: &frontmatter.id,
            slug: &frontmatter.slug,
            vault_path: &frontmatter.vault_path,
            published: frontmatter.published_at.is_some(),
        }
    }
    fn row_view(row: &Self::Row) -> RowView<'_> {
        RowView {
            id: &row.id,
            slug: &row.slug,
            name: &row.name,
            vault_path: &row.vault_path,
            created_at: &row.created_at,
            updated_at: &row.updated_at,
        }
    }
    async fn upsert(&self, database: &db::Database, row: &Self::Row) -> Result<(), String> {
        self.0.upsert(database, row).await
    }
    async fn list_all(&self, database: &db::Database) -> Result<Vec<Self::Row>, String> {
        self.0.list_all(database).await
    }
    async fn delete_by_id(&self, database: &db::Database, id: &str) -> Result<(), String> {
        self.0.delete_by_id(database, id).await
    }
}

struct ItemSync<'a>(&'a dyn ItemRepository);

#[async_trait]
impl SyncRepository for ItemSync<'_> {
    type Frontmatter = ItemFrontmatter;
    type Row = db::ItemRow;
    const KIND: &'static str = "item";

    fn list_store(&self, store: &EntityStore) -> Result<Vec<Self::Frontmatter>, String> {
        store.list_items().map_err(|err| err.to_string())
    }
    fn delete_from_store(&self, store: &EntityStore, slug: &str) -> Result<(), String> {
        store.delete_item(slug).map_err(|err| err.to_string())
    }
    fn row_from_frontmatter(frontmatter: &Self::Frontmatter) -> Result<Self::Row, String> {
        item_row_from_frontmatter(frontmatter)
    }
    fn frontmatter_view(frontmatter: &Self::Frontmatter) -> StoreView<'_> {
        StoreView {
            id: &frontmatter.id,
            slug: &frontmatter.slug,
            vault_path: &frontmatter.vault_path,
            published: frontmatter.published_at.is_some(),
        }
    }
    fn row_view(row: &Self::Row) -> RowView<'_> {
        RowView {
            id: &row.id,
            slug: &row.slug,
            name: &row.name,
            vault_path: &row.vault_path,
            created_at: &row.created_at,
            updated_at: &row.updated_at,
        }
    }
    async fn upsert(&self, database: &db::Database, row: &Self::Row) -> Result<(), String> {
        self.0.upsert(database, row).await
    }
    async fn list_all(&self, database: &db::Database) -> Result<Vec<Self::Row>, String> {
        self.0.list_all(database).await
    }
    async fn delete_by_id(&self, database: &db::Database, id: &str) -> Result<(), String> {
        self.0.delete_by_id(database, id).await
    }
}

struct EventSync<'a>(&'a dyn EventRepository);

#[async_trait]
impl SyncRepository for EventSync<'_> {
    type Frontmatter = EventFrontmatter;
    type Row = db::EventRow;
    const KIND: &'static str = "event";

    fn list_store(&self, store: &EntityStore) -> Result<Vec<Self::Frontmatter>, String> {
        store.list_events().map_err(|err| err.to_string())
    }
    fn delete_from_store(&self, store: &EntityStore, slug: &str) -> Result<(), String> {
        store.delete_event(slug).map_err(|err| err.to_string())
    }
    fn row_from_frontmatter(frontmatter: &Self::Frontmatter) -> Result<Self::Row, String> {
        event_row_from_frontmatter(frontmatter)
    }
    fn frontmatter_view(frontmatter: &Self::Frontmatter) -> StoreView<'_> {
        StoreView {
            id: &frontmatter.id,
            slug: &frontmatter.slug,
            vault_path: &frontmatter.vault_path,
            published: frontmatter.published_at.is_some(),
        }
    }
    fn row_view(row: &Self::Row) -> RowView<'_> {
        RowView {
            id: &row.id,
            slug: &row.slug,
            name: &row.name,
            vault_path: &row.vault_path,
            created_at: &row.created_at,
            updated_at: &row.updated_at,
        }
    }
    async fn upsert(&self, database: &db::Database, row: &Self::Row) -> Result<(), String> {
        self.0.upsert(database, row).await
    }
    async fn list_all(&self, database: &db::Database) -> Result<Vec<Self::Row>, String> {
        self.0.list_all(database).await
    }
    async fn delete_by_id(&self, database: &db::Database, id: &str) -> Result<(), String> {
        self.0.delete_by_id(database, id).await
    }
}

struct GodSync<'a>(&'a dyn GodRepository);

#[async_trait]
impl SyncRepository for GodSync<'_> {
    type Frontmatter = GodFrontmatter;
    type Row = db::GodRow;
    const KIND: &'static str = "god";

    fn list_store(&self, store: &EntityStore) -> Result<Vec<Self::Frontmatter>, String> {
        store.list_gods().map_err(|err| err.to_string())
    }
    fn delete_from_store(&self, store: &EntityStore, slug: &str) -> Result<(), String> {
        store.delete_god(slug).map_err(|err| err.to_string())
    }
    fn row_from_frontmatter(frontmatter: &Self::Frontmatter) -> Result<Self::Row, String> {
        god_row_from_frontmatter(frontmatter)
    }
    fn frontmatter_view(frontmatter: &Self::Frontmatter) -> StoreView<'_> {
        StoreView {
            id: &frontmatter.id,
            slug: &frontmatter.slug,
            vault_path: &frontmatter.vault_path,
            published: frontmatter.published_at.is_some(),
        }
    }
    fn row_view(row: &Self::Row) -> RowView<'_> {
        RowView {
            id: &row.id,
            slug: &row.slug,
            name: &row.name,
            vault_path: &row.vault_path,
            created_at: &row.created_at,
            updated_at: &row.updated_at,
        }
    }
    async fn upsert(&self, database: &db::Database, row: &Self::Row) -> Result<(), String> {
        self.0.upsert(database, row).await
    }
    async fn list_all(&self, database: &db::Database) -> Result<Vec<Self::Row>, String> {
        self.0.list_all(database).await
    }
    async fn delete_by_id(&self, database: &db::Database, id: &str) -> Result<(), String> {
        self.0.delete_by_id(database, id).await
    }
}

struct DungeonSync<'a>(&'a dyn DungeonRepository);

#[async_trait]
impl SyncRepository for DungeonSync<'_> {
    type Frontmatter = DungeonFrontmatter;
    type Row = db::DungeonRow;
    const KIND: &'static str = "dungeon";

    fn list_store(&self, store: &EntityStore) -> Result<Vec<Self::Frontmatter>, String> {
        store.list_dungeons().map_err(|err| err.to_string())
    }
    fn delete_from_store(&self, store: &EntityStore, slug: &str) -> Result<(), String> {
        store.delete_dungeon(slug).map_err(|err| err.to_string())
    }
    fn row_from_frontmatter(frontmatter: &Self::Frontmatter) -> Result<Self::Row, String> {
        dungeon_row_from_frontmatter(frontmatter)
    }
    fn frontmatter_view(frontmatter: &Self::Frontmatter) -> StoreView<'_> {
        StoreView {
            id: &frontmatter.id,
            slug: &frontmatter.slug,
            vault_path: &frontmatter.vault_path,
            published: frontmatter.published_at.is_some(),
        }
    }
    fn row_view(row: &Self::Row) -> RowView<'_> {
        RowView {
            id: &row.id,
            slug: &row.slug,
            name: &row.name,
            vault_path: &row.vault_path,
            created_at: &row.created_at,
            updated_at: &row.updated_at,
        }
    }
    async fn upsert(&self, database: &db::Database, row: &Self::Row) -> Result<(), String> {
        self.0.upsert(database, row).await
    }
    async fn list_all(&self, database: &db::Database) -> Result<Vec<Self::Row>, String> {
        self.0.list_all(database).await
    }
    async fn delete_by_id(&self, database: &db::Database, id: &str) -> Result<(), String> {
        self.0.delete_by_id(database, id).await
    }
}

pub(crate) fn dungeon_row_from_frontmatter(
    frontmatter: &DungeonFrontmatter,
) -> Result<db::DungeonRow, String> {
    let beats_json = serde_json::to_string(&frontmatter.beats)
        .map_err(|err| format!("failed to encode dungeon beats: {err}"))?;
    Ok(db::DungeonRow {
        id: frontmatter.id.clone(),
        slug: frontmatter.slug.clone(),
        name: frontmatter.name.clone(),
        vault_path: frontmatter.vault_path.clone(),
        location: frontmatter.location.clone(),
        story: frontmatter.story.clone(),
        premise: frontmatter.premise.clone(),
        topology: frontmatter.topology.clone(),
        tone: frontmatter.tone.clone(),
        twist: frontmatter.twist.clone(),
        beats_json,
        created_at: frontmatter.created_at.clone(),
        updated_at: frontmatter.updated_at.clone(),
    })
}

pub(crate) fn npc_row_from_frontmatter(frontmatter: &NpcFrontmatter) -> Result<db::NpcRow, String> {
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
        carrying: carrying_to_db_text(&frontmatter.carrying).map_err(|err| err.to_string())?,
        location: frontmatter.location.clone(),
        vault_path: frontmatter.vault_path.clone(),
        created_at: frontmatter.created_at.clone(),
        updated_at: frontmatter.updated_at.clone(),
    })
}

pub(crate) fn location_row_from_frontmatter(
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

pub(crate) fn faction_row_from_frontmatter(
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

pub(crate) fn item_row_from_frontmatter(
    frontmatter: &ItemFrontmatter,
) -> Result<db::ItemRow, String> {
    Ok(db::ItemRow {
        id: frontmatter.id.clone(),
        slug: frontmatter.slug.clone(),
        name: frontmatter.name.clone(),
        vault_path: frontmatter.vault_path.clone(),
        category: frontmatter.category.clone(),
        rarity: frontmatter.rarity.clone(),
        attunement: frontmatter.attunement.clone(),
        materials: faction_list_to_db_text(&frontmatter.materials)
            .map_err(|err| err.to_string())?,
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

pub(crate) fn event_row_from_frontmatter(
    frontmatter: &EventFrontmatter,
) -> Result<db::EventRow, String> {
    // Events carry no list/serialized columns, so this is a straight field copy.
    Ok(db::EventRow {
        id: frontmatter.id.clone(),
        slug: frontmatter.slug.clone(),
        name: frontmatter.name.clone(),
        vault_path: frontmatter.vault_path.clone(),
        body: frontmatter.body.clone(),
        created_at: frontmatter.created_at.clone(),
        updated_at: frontmatter.updated_at.clone(),
    })
}

pub(crate) fn god_row_from_frontmatter(frontmatter: &GodFrontmatter) -> Result<db::GodRow, String> {
    Ok(db::GodRow {
        id: frontmatter.id.clone(),
        slug: frontmatter.slug.clone(),
        name: frontmatter.name.clone(),
        vault_path: frontmatter.vault_path.clone(),
        epithet: frontmatter.epithet.clone(),
        rank: frontmatter.rank.clone(),
        rank_custom: frontmatter.rank_custom.clone(),
        alignment: frontmatter.alignment.clone(),
        domains: faction_list_to_db_text(&frontmatter.domains).map_err(|err| err.to_string())?,
        symbol: frontmatter.symbol.clone(),
        appearance: frontmatter.appearance.clone(),
        dogma: frontmatter.dogma.clone(),
        realm: frontmatter.realm.clone(),
        worshippers: frontmatter.worshippers.clone(),
        clergy: frontmatter.clergy.clone(),
        allies: faction_list_to_db_text(&frontmatter.allies).map_err(|err| err.to_string())?,
        rivals: faction_list_to_db_text(&frontmatter.rivals).map_err(|err| err.to_string())?,
        created_at: frontmatter.created_at.clone(),
        updated_at: frontmatter.updated_at.clone(),
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
            published_at: None,
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
            published_at: None,
        };

        let row = location_row_from_frontmatter(&frontmatter).expect("row");
        assert!(row.exports.contains("Incense"));
        assert_eq!(row.kind_custom.as_deref(), Some("Sanctum"));
    }
}
