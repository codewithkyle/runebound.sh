use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use sqlx::migrate::Migrator;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Row, SqlitePool};

use crate::db_macros::impl_entity_table;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
const APP_DIR_NAME: &str = "runebound.sh";

pub struct Database {
    pub pool: SqlitePool,
    pub path: PathBuf,
}

/// A SQLite transaction over this app's pool, vended by [`Database::begin`] and
/// threaded through the executor-generic mutations + the repository `*_tx` methods
/// so a compound DB change commits atomically (P6.1). Re-exported so the desktop
/// crate need not depend on `sqlx` directly.
pub type DbTransaction<'a> = sqlx::Transaction<'a, sqlx::Sqlite>;

impl Database {
    /// Begin a transaction for an atomic multi-statement unit of work — the basis
    /// for committing a save's DB projection (row + document index) or a
    /// soft-delete / reap's deletes as a unit, so a mid-sequence failure can't
    /// leave the index out of step with its row (P6.1). The canonical TOML store is
    /// written outside the transaction (it is the source of truth a partial DB
    /// failure self-heals from on the next `sync`).
    pub async fn begin(&self) -> Result<sqlx::Transaction<'_, sqlx::Sqlite>> {
        self.pool
            .begin()
            .await
            .context("failed to begin database transaction")
    }
}

#[derive(Debug, Clone)]
pub struct LocationRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub kind_type: String,
    pub kind_custom: Option<String>,
    pub visual_description: String,
    pub history_background: String,
    pub exports: String,
    pub tone: String,
    pub authority: String,
    pub danger_level: String,
    pub current_tension: String,
    /// The location this one stands within (a guildhall's containing place). Empty
    /// when there is no anchor.
    pub location: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct FactionRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub kind_type: String,
    /// Derived from `kind_type` at save (D2): "houses" | "establishments" |
    /// "religion" (or "" for a drifted/unknown kind). `NOT NULL`.
    pub category: String,
    // Visible face.
    pub public_description: String,
    pub reputation: String,
    pub symbol_description: String,
    // WOAC engine (design §5).
    pub want: String,
    pub obstacle: String,
    pub action: String,
    pub consequence: String,
    /// Was `leadership`. Picker-linked or blank, never LLM-generated (D3).
    pub leader: String,
    pub sphere_of_influence: String,
    pub resources_assets: String,
    pub allies: String,
    pub rivals_enemies: String,
    /// Houses Vassal/Lord only — NULL for everything else.
    pub liege: Option<String>,
    pub loyalty_type: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct GodRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub epithet: String,
    pub rank: String,
    pub rank_custom: Option<String>,
    pub alignment: String,
    pub domains: String,
    pub symbol: String,
    pub appearance: String,
    pub dogma: String,
    pub realm: String,
    pub worshippers: String,
    pub clergy: String,
    pub allies: String,
    pub rivals: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct DungeonRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub location: String,
    pub story: String,
    pub premise: String,
    pub topology: String,
    pub tone: String,
    pub twist: String,
    pub beats_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ItemRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub category: String,
    pub rarity: String,
    pub attunement: String,
    pub materials: String,
    pub appearance: String,
    pub abilities: String,
    pub drawbacks: String,
    pub history: String,
    pub value: String,
    pub location: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct EventRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NpcRow {
    pub id: String,
    pub slug: String,
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
    pub carrying: String,
    pub location: String,
    pub vault_path: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct SpellRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub level: i64,
    pub school: String,
    pub source: String,
    pub ritual: bool,
    pub concentration: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct MonsterRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub cr: String,
    /// Numeric CR for ordering ("1/4" -> 0.25).
    pub cr_sort: f64,
    pub creature_type: String,
    pub size: String,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct SoftDeleteRow {
    pub id: i64,
    pub entity_type: String,
    pub entity_id: String,
    pub name: String,
    pub slug: String,
    pub original_vault_path: String,
    pub trash_vault_path: String,
    pub payload_json: String,
    pub created_at: String,
    pub undone_at: Option<String>,
    pub operation: String,
}

impl_entity_table! {
    table: "npcs",
    row: NpcRow,
    upsert: upsert_npc,
    find_by_id: find_npc_by_id,
    find_by_slug: find_npc_by_slug,
    find_by_name_or_slug: find_npc_by_name_or_slug,
    list: list_npcs,
    search_by_name: search_npcs_by_name,
    delete_by_id: delete_npc_by_id,
    row_to: row_to_npc,
    columns: [
        strict slug,
        strict name,
        strict race,
        lenient occupation = "Unknown".to_string(),
        strict sex,
        lenient age = "Unknown".to_string(),
        lenient height = "Unknown".to_string(),
        lenient weight_lbs = "Unknown".to_string(),
        lenient background = "Unknown".to_string(),
        lenient want_need = "Unknown".to_string(),
        lenient secret_obstacle = "Unknown".to_string(),
        lenient carrying = "[\"Unknown\"]".to_string(),
        strict location,
        strict vault_path,
    ],
}

impl_entity_table! {
    table: "locations",
    row: LocationRow,
    upsert: upsert_location,
    find_by_id: find_location_by_id,
    find_by_slug: find_location_by_slug,
    find_by_name_or_slug: find_location_by_name_or_slug,
    list: list_locations,
    search_by_name: search_locations_by_name,
    delete_by_id: delete_location_by_id,
    row_to: row_to_location,
    columns: [
        strict slug,
        strict name,
        strict vault_path,
        lenient kind_type = "other".to_string(),
        opt kind_custom,
        lenient visual_description = "Unknown".to_string(),
        lenient history_background = "Unknown".to_string(),
        lenient exports = "[\"Unknown\"]".to_string(),
        lenient tone = "Unknown".to_string(),
        lenient authority = "Unknown".to_string(),
        lenient danger_level = "Unknown".to_string(),
        lenient current_tension = "Unknown".to_string(),
        lenient location = String::new(),
    ],
}

impl_entity_table! {
    table: "factions",
    row: FactionRow,
    upsert: upsert_faction,
    find_by_id: find_faction_by_id,
    find_by_slug: find_faction_by_slug,
    find_by_name_or_slug: find_faction_by_name_or_slug,
    list: list_factions,
    search_by_name: search_factions_by_name,
    delete_by_id: delete_faction_by_id,
    row_to: row_to_faction,
    columns: [
        strict slug,
        strict name,
        strict vault_path,
        // No `other` kind exists (9 fixed kinds), so a drifted row falls back to "".
        lenient kind_type = String::new(),
        lenient category = String::new(),
        lenient public_description = "Unknown".to_string(),
        lenient reputation = "Unknown".to_string(),
        lenient symbol_description = "Unknown".to_string(),
        lenient want = "Unknown".to_string(),
        lenient obstacle = "Unknown".to_string(),
        lenient action = "Unknown".to_string(),
        lenient consequence = "Unknown".to_string(),
        // Relational fields are blank when unlinked (D3), not "Unknown".
        lenient leader = String::new(),
        lenient sphere_of_influence = "Unknown".to_string(),
        lenient resources_assets = "[\"Unknown\"]".to_string(),
        lenient allies = "[]".to_string(),
        lenient rivals_enemies = "[]".to_string(),
        opt liege,
        opt loyalty_type,
    ],
}

impl_entity_table! {
    table: "gods",
    row: GodRow,
    upsert: upsert_god,
    find_by_id: find_god_by_id,
    find_by_slug: find_god_by_slug,
    find_by_name_or_slug: find_god_by_name_or_slug,
    list: list_gods,
    search_by_name: search_gods_by_name,
    delete_by_id: delete_god_by_id,
    row_to: row_to_god,
    columns: [
        strict slug,
        strict name,
        strict vault_path,
        lenient epithet = "Unknown".to_string(),
        lenient rank = "other".to_string(),
        opt rank_custom,
        lenient alignment = "TN".to_string(),
        lenient domains = "[\"Unknown\"]".to_string(),
        lenient symbol = "Unknown".to_string(),
        lenient appearance = "Unknown".to_string(),
        lenient dogma = "Unknown".to_string(),
        lenient realm = "Unknown".to_string(),
        lenient worshippers = "Unknown".to_string(),
        lenient clergy = "Unknown".to_string(),
        lenient allies = "[\"Unknown\"]".to_string(),
        lenient rivals = "[\"Unknown\"]".to_string(),
    ],
}

impl_entity_table! {
    table: "dungeons",
    row: DungeonRow,
    upsert: upsert_dungeon,
    find_by_id: find_dungeon_by_id,
    find_by_slug: find_dungeon_by_slug,
    find_by_name_or_slug: find_dungeon_by_name_or_slug,
    list: list_dungeons,
    search_by_name: search_dungeons_by_name,
    delete_by_id: delete_dungeon_by_id,
    row_to: row_to_dungeon,
    columns: [
        strict slug,
        strict name,
        strict vault_path,
        lenient location = String::new(),
        lenient story = String::new(),
        lenient premise = "Unknown".to_string(),
        lenient topology = "none".to_string(),
        lenient tone = "tragedy".to_string(),
        lenient twist = "neither".to_string(),
        lenient beats_json = "[]".to_string(),
    ],
}

impl_entity_table! {
    table: "items",
    row: ItemRow,
    upsert: upsert_item,
    find_by_id: find_item_by_id,
    find_by_slug: find_item_by_slug,
    find_by_name_or_slug: find_item_by_name_or_slug,
    list: list_items,
    search_by_name: search_items_by_name,
    delete_by_id: delete_item_by_id,
    row_to: row_to_item,
    columns: [
        strict slug,
        strict name,
        strict vault_path,
        lenient category = "other".to_string(),
        lenient rarity = "unknown".to_string(),
        lenient attunement = "Unknown".to_string(),
        lenient materials = "[\"Unknown\"]".to_string(),
        lenient appearance = "Unknown".to_string(),
        lenient abilities = "Unknown".to_string(),
        lenient drawbacks = "Unknown".to_string(),
        lenient history = "Unknown".to_string(),
        lenient value = "Unknown".to_string(),
        lenient location = "Unknown".to_string(),
    ],
}

impl_entity_table! {
    table: "events",
    row: EventRow,
    upsert: upsert_event,
    find_by_id: find_event_by_id,
    find_by_slug: find_event_by_slug,
    find_by_name_or_slug: find_event_by_name_or_slug,
    list: list_events,
    search_by_name: search_events_by_name,
    delete_by_id: delete_event_by_id,
    row_to: row_to_event,
    columns: [
        strict slug,
        strict name,
        strict vault_path,
        strict body,
    ],
}

// Spells are a read-only reference library imported from the user's own 5etools
// copy, so they have no `vault_path` (never published) and the full set is replaced
// wholesale on re-import (see `clear_spells`). The generated `search_spells_by_name`
// is the LIKE-based typeahead — 554 rows, no FTS5 needed.
impl_entity_table! {
    table: "spells",
    row: SpellRow,
    upsert: upsert_spell,
    find_by_id: find_spell_by_id,
    find_by_slug: find_spell_by_slug,
    find_by_name_or_slug: find_spell_by_name_or_slug,
    list: list_spells,
    search_by_name: search_spells_by_name,
    delete_by_id: delete_spell_by_id,
    row_to: row_to_spell,
    columns: [
        strict slug,
        strict name,
        strict level,
        strict school,
        strict source,
        strict ritual,
        strict concentration,
    ],
}

/// Remove every spell row. Executor-generic so a re-import can clear + repopulate
/// the table in one transaction (the canonical TOML store is the source of truth).
pub async fn clear_spells<'e, E>(executor: E) -> Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query("DELETE FROM spells")
        .execute(executor)
        .await
        .context("failed to clear spells")?;
    Ok(())
}

/// Count of imported spells — drives the import summary and the "did you run
/// spellbook import?" empty-library hint.
pub async fn count_spells(pool: &SqlitePool) -> Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM spells")
        .fetch_one(pool)
        .await
        .context("failed to count spells")?;
    Ok(row.try_get("n").unwrap_or(0))
}

// Monsters are a read-only reference library imported from the user's own 5etools
// copy (like spells): no `vault_path`, replaced wholesale on re-import (see
// `clear_monsters`). The generated `search_monsters_by_name` is the LIKE-based
// typeahead — ~2575 rows, no FTS5 needed.
impl_entity_table! {
    table: "monsters",
    row: MonsterRow,
    upsert: upsert_monster,
    find_by_id: find_monster_by_id,
    find_by_slug: find_monster_by_slug,
    find_by_name_or_slug: find_monster_by_name_or_slug,
    list: list_monsters,
    search_by_name: search_monsters_by_name,
    delete_by_id: delete_monster_by_id,
    row_to: row_to_monster,
    columns: [
        strict slug,
        strict name,
        strict cr,
        strict cr_sort,
        strict creature_type,
        strict size,
        strict source,
    ],
}

/// Remove every monster row. Executor-generic so a re-import can clear + repopulate
/// the table in one transaction (the canonical TOML store is the source of truth).
pub async fn clear_monsters<'e, E>(executor: E) -> Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query("DELETE FROM monsters")
        .execute(executor)
        .await
        .context("failed to clear monsters")?;
    Ok(())
}

/// Count of imported monsters — drives the import summary and the "did you run
/// bestiary import?" empty-library hint.
pub async fn count_monsters(pool: &SqlitePool) -> Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM monsters")
        .fetch_one(pool)
        .await
        .context("failed to count monsters")?;
    Ok(row.try_get("n").unwrap_or(0))
}

pub async fn init_database() -> Result<Database> {
    let path = default_database_path()?;
    init_database_at_path(&path).await
}

pub async fn init_database_at_path(path: &Path) -> Result<Database> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create db directory {}", parent.display()))?;
    }

    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .disable_statement_logging();

    let pool = SqlitePool::connect_with(options)
        .await
        .with_context(|| format!("failed to connect to sqlite database at {}", path.display()))?;

    MIGRATOR
        .run(&pool)
        .await
        .context("failed to run sqlite migrations")?;

    Ok(Database {
        pool,
        path: path.to_path_buf(),
    })
}

pub async fn health_check(pool: &SqlitePool) -> Result<()> {
    let row = sqlx::query("SELECT 1 AS ok")
        .fetch_one(pool)
        .await
        .context("sqlite health check query failed")?;
    let ok: i64 = row
        .try_get("ok")
        .context("sqlite health check row invalid")?;

    if ok != 1 {
        return Err(anyhow!("sqlite health check returned unexpected value"));
    }

    Ok(())
}

pub fn default_database_path() -> Result<PathBuf> {
    let data_dir = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .ok_or_else(|| anyhow!("unable to resolve local data directory"))?;
    Ok(data_dir.join(APP_DIR_NAME).join("app.db"))
}

pub async fn find_document_by_vault_path(
    pool: &SqlitePool,
    vault_path: &str,
) -> Result<Option<String>> {
    let row = sqlx::query("SELECT slug FROM documents WHERE vault_path = ?1")
        .bind(vault_path)
        .fetch_optional(pool)
        .await
        .context("failed to find document by vault path")?;

    Ok(row.map(|r| r.get::<String, _>("slug")))
}

// Executor-generic (pool or `&mut Transaction`) so the document index can be
// upserted in the same transaction as its entity row (P6.1).
pub async fn upsert_document_index<'e, E>(
    executor: E,
    doc_type: &str,
    slug: &str,
    title: Option<&str>,
    vault_path: &str,
    created_at: &str,
    updated_at: &str,
) -> Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query(
        "INSERT INTO documents (doc_type, slug, title, vault_path, tags, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6)
         ON CONFLICT(vault_path) DO UPDATE SET
            doc_type = excluded.doc_type,
            slug = excluded.slug,
            title = excluded.title,
            updated_at = excluded.updated_at,
            indexed_at = datetime('now')",
    )
    .bind(doc_type)
    .bind(slug)
    .bind(title)
    .bind(vault_path)
    .bind(created_at)
    .bind(updated_at)
    .execute(executor)
    .await
    .context("failed to upsert documents index")?;

    Ok(())
}

// Executor-generic (pool or `&mut Transaction`) — see `upsert_document_index` (P6.1).
pub async fn delete_document_by_vault_path<'e, E>(executor: E, vault_path: &str) -> Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query("DELETE FROM documents WHERE vault_path = ?1")
        .bind(vault_path)
        .execute(executor)
        .await
        .context("failed to delete document index row")?;

    Ok(())
}

pub async fn insert_soft_delete(pool: &SqlitePool, row: &SoftDeleteRow) -> Result<i64> {
    let result = sqlx::query(
        "INSERT INTO soft_deletes (
            entity_type,
            entity_id,
            name,
            slug,
            original_vault_path,
            trash_vault_path,
            payload_json,
            created_at,
            undone_at,
            operation
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(&row.entity_type)
    .bind(&row.entity_id)
    .bind(&row.name)
    .bind(&row.slug)
    .bind(&row.original_vault_path)
    .bind(&row.trash_vault_path)
    .bind(&row.payload_json)
    .bind(&row.created_at)
    .bind(&row.undone_at)
    .bind(&row.operation)
    .execute(pool)
    .await
    .context("failed to insert soft delete row")?;

    Ok(result.last_insert_rowid())
}

pub async fn latest_pending_soft_delete(pool: &SqlitePool) -> Result<Option<SoftDeleteRow>> {
    let row = sqlx::query(
        "SELECT id, entity_type, entity_id, name, slug, original_vault_path, trash_vault_path, payload_json, created_at, undone_at, operation
         FROM soft_deletes
         WHERE undone_at IS NULL
         ORDER BY id DESC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .context("failed to query latest pending soft delete")?;

    row.map(row_to_soft_delete).transpose()
}

pub async fn mark_soft_delete_undone(pool: &SqlitePool, id: i64, undone_at: &str) -> Result<()> {
    sqlx::query("UPDATE soft_deletes SET undone_at = ?2 WHERE id = ?1")
        .bind(id)
        .bind(undone_at)
        .execute(pool)
        .await
        .context("failed to mark soft delete as undone")?;

    Ok(())
}

/// Finalizes any still-pending `publish` recovery records so they can no longer be
/// undone (a publish that survives to a restart is permanent). Returns the count.
pub async fn finalize_pending_publishes(pool: &SqlitePool, finalized_at: &str) -> Result<u64> {
    let result = sqlx::query(
        "UPDATE soft_deletes SET undone_at = ?1 WHERE operation = 'publish' AND undone_at IS NULL",
    )
    .bind(finalized_at)
    .execute(pool)
    .await
    .context("failed to finalize pending publishes")?;

    Ok(result.rows_affected())
}

pub async fn insert_generation(
    pool: &SqlitePool,
    entity_type: &str,
    entity_id: Option<&str>,
    prompt: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO generations (entity_type, entity_id, prompt)
         VALUES (?1, ?2, ?3)",
    )
    .bind(entity_type)
    .bind(entity_id)
    .bind(prompt)
    .execute(pool)
    .await
    .context("failed to insert generation row")?;

    Ok(())
}

pub async fn recent_generation_prompts(
    pool: &SqlitePool,
    entity_type: &str,
    limit: i64,
) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT prompt
         FROM generations
         WHERE entity_type = ?1
         ORDER BY id DESC
         LIMIT ?2",
    )
    .bind(entity_type)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to query recent generations")?;

    rows.into_iter()
        .map(|row| row.try_get("prompt").context("generations.prompt missing"))
        .collect()
}

fn row_to_soft_delete(row: sqlx::sqlite::SqliteRow) -> Result<SoftDeleteRow> {
    Ok(SoftDeleteRow {
        id: row.try_get("id").context("soft_deletes.id missing")?,
        entity_type: row
            .try_get("entity_type")
            .context("soft_deletes.entity_type missing")?,
        entity_id: row
            .try_get("entity_id")
            .context("soft_deletes.entity_id missing")?,
        name: row.try_get("name").context("soft_deletes.name missing")?,
        slug: row.try_get("slug").context("soft_deletes.slug missing")?,
        original_vault_path: row
            .try_get("original_vault_path")
            .context("soft_deletes.original_vault_path missing")?,
        trash_vault_path: row
            .try_get("trash_vault_path")
            .context("soft_deletes.trash_vault_path missing")?,
        payload_json: row
            .try_get("payload_json")
            .context("soft_deletes.payload_json missing")?,
        created_at: row
            .try_get("created_at")
            .context("soft_deletes.created_at missing")?,
        undone_at: row.try_get("undone_at").ok(),
        operation: row
            .try_get("operation")
            .context("soft_deletes.operation missing")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn temp_db() -> Database {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("dnd_db_test_{}_{}.sqlite", std::process::id(), n));
        let _ = std::fs::remove_file(&path);
        init_database_at_path(&path).await.expect("init test db")
    }

    fn sample_row(entity_id: &str, operation: &str) -> SoftDeleteRow {
        SoftDeleteRow {
            id: 0,
            entity_type: "npc".to_string(),
            entity_id: entity_id.to_string(),
            name: "Jimmy".to_string(),
            slug: "jimmy".to_string(),
            original_vault_path: "npcs/Jimmy.md".to_string(),
            trash_vault_path: String::new(),
            payload_json: "{}".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            undone_at: None,
            operation: operation.to_string(),
        }
    }

    #[tokio::test]
    async fn operation_round_trips_and_finalize_consumes_publishes() {
        let database = temp_db().await;
        let pool = &database.pool;

        insert_soft_delete(pool, &sample_row("npc_1", "delete"))
            .await
            .expect("insert delete");
        insert_soft_delete(pool, &sample_row("npc_2", "publish"))
            .await
            .expect("insert publish");

        // Latest pending is the publish (highest id); operation round-trips.
        let latest = latest_pending_soft_delete(pool)
            .await
            .expect("query")
            .expect("a pending row");
        assert_eq!(latest.operation, "publish");
        assert_eq!(latest.entity_id, "npc_2");

        // Finalizing consumes exactly the pending publish, not the delete.
        let finalized = finalize_pending_publishes(pool, "2026-06-15T01:00:00Z")
            .await
            .expect("finalize");
        assert_eq!(finalized, 1);

        // The delete record is now the only thing left to undo.
        let latest = latest_pending_soft_delete(pool)
            .await
            .expect("query")
            .expect("a pending row");
        assert_eq!(latest.operation, "delete");
        assert_eq!(latest.entity_id, "npc_1");
    }

    fn sample_npc(id: &str, name: &str, slug: &str) -> NpcRow {
        NpcRow {
            id: id.to_string(),
            slug: slug.to_string(),
            name: name.to_string(),
            race: "Human".to_string(),
            occupation: "Town Guard".to_string(),
            sex: "female".to_string(),
            age: "42".to_string(),
            height: "5'11\"".to_string(),
            weight_lbs: "185".to_string(),
            background: "Former caravan guard.".to_string(),
            want_need: "Coin to leave town.".to_string(),
            secret_obstacle: "Owes a smuggler a blood debt.".to_string(),
            carrying: "keys, ledger, hidden dagger".to_string(),
            location: "Waterdeep".to_string(),
            vault_path: format!("npcs/{name}.md"),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T00:00:00Z".to_string(),
        }
    }

    fn sample_location(id: &str, name: &str, slug: &str) -> LocationRow {
        LocationRow {
            id: id.to_string(),
            slug: slug.to_string(),
            name: name.to_string(),
            vault_path: format!("locations/{name}.md"),
            kind_type: "city".to_string(),
            kind_custom: None,
            visual_description: "Lantern-lit markets line flooded alleys.".to_string(),
            history_background: "Built on drowned ruins.".to_string(),
            exports: "smoked eel, river pearls".to_string(),
            tone: "damp suspicious".to_string(),
            authority: "Merchants' Compact".to_string(),
            danger_level: "risky".to_string(),
            current_tension: "Guild war brews.".to_string(),
            location: "Saltmarsh".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn npc_upsert_round_trips_every_field() {
        let database = temp_db().await;
        let pool = &database.pool;
        let npc = sample_npc("npc_1", "Lirael Drake", "lirael-drake");

        upsert_npc(pool, &npc).await.expect("upsert npc");
        let found = find_npc_by_id(pool, "npc_1")
            .await
            .expect("query")
            .expect("npc present");

        assert_eq!(found.name, "Lirael Drake");
        assert_eq!(found.slug, "lirael-drake");
        assert_eq!(found.occupation, "Town Guard");
        // The carrying list is stored as a single column; confirm it survives intact.
        assert_eq!(found.carrying, "keys, ledger, hidden dagger");
        assert_eq!(found.vault_path, "npcs/Lirael Drake.md");
    }

    #[tokio::test]
    async fn npc_find_by_name_or_slug_matches_both_and_is_case_insensitive() {
        let database = temp_db().await;
        let pool = &database.pool;
        upsert_npc(pool, &sample_npc("npc_1", "Lirael Drake", "lirael-drake"))
            .await
            .expect("upsert");

        // Name match, ignoring case.
        assert_eq!(
            find_npc_by_name_or_slug(pool, "LIRAEL DRAKE")
                .await
                .expect("query")
                .map(|n| n.id),
            Some("npc_1".to_string()),
        );
        // Slug match.
        assert_eq!(
            find_npc_by_name_or_slug(pool, "lirael-drake")
                .await
                .expect("query")
                .map(|n| n.id),
            Some("npc_1".to_string()),
        );
        // Unknown input resolves to nothing.
        assert!(
            find_npc_by_name_or_slug(pool, "nobody")
                .await
                .expect("query")
                .is_none()
        );
    }

    #[tokio::test]
    async fn npc_upsert_updates_existing_row_on_id_conflict() {
        let database = temp_db().await;
        let pool = &database.pool;

        upsert_npc(pool, &sample_npc("npc_1", "Lirael Drake", "lirael-drake"))
            .await
            .expect("insert");

        // Re-upsert the same id with a new name + later timestamp.
        let mut renamed = sample_npc("npc_1", "Lirael the Bold", "lirael-the-bold");
        renamed.created_at = "2099-01-01T00:00:00Z".to_string(); // should be ignored on conflict
        renamed.updated_at = "2026-06-16T00:00:00Z".to_string();
        upsert_npc(pool, &renamed).await.expect("update");

        // Still a single row — the conflict updated rather than duplicated.
        let all = list_npcs(pool).await.expect("list");
        assert_eq!(all.len(), 1);

        let found = find_npc_by_id(pool, "npc_1")
            .await
            .expect("query")
            .expect("present");
        assert_eq!(found.name, "Lirael the Bold");
        assert_eq!(found.updated_at, "2026-06-16T00:00:00Z");
        // ON CONFLICT does not touch created_at, so the original is preserved.
        assert_eq!(found.created_at, "2026-06-15T00:00:00Z");
    }

    #[tokio::test]
    async fn search_npcs_by_name_is_substring_case_insensitive_and_sorted() {
        let database = temp_db().await;
        let pool = &database.pool;
        upsert_npc(
            pool,
            &sample_npc("npc_1", "Bram Stoneford", "bram-stoneford"),
        )
        .await
        .expect("upsert");
        upsert_npc(pool, &sample_npc("npc_2", "Aldric Vane", "aldric-vane"))
            .await
            .expect("upsert");
        upsert_npc(pool, &sample_npc("npc_3", "Branwen Ash", "branwen-ash"))
            .await
            .expect("upsert");

        // Mixed-case substring "BR" matches Bram + Branwen, returned name-sorted.
        let results = search_npcs_by_name(pool, "BR", 10).await.expect("search");
        let names: Vec<String> = results.into_iter().map(|n| n.name).collect();
        assert_eq!(names, vec!["Bram Stoneford", "Branwen Ash"]);
    }

    #[tokio::test]
    async fn search_npcs_by_name_respects_limit() {
        let database = temp_db().await;
        let pool = &database.pool;
        for i in 0..5 {
            upsert_npc(
                pool,
                &sample_npc(
                    &format!("npc_{i}"),
                    &format!("Guard {i}"),
                    &format!("guard-{i}"),
                ),
            )
            .await
            .expect("upsert");
        }

        let results = search_npcs_by_name(pool, "guard", 3).await.expect("search");
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn delete_npc_by_id_removes_only_the_targeted_row() {
        let database = temp_db().await;
        let pool = &database.pool;
        upsert_npc(pool, &sample_npc("npc_1", "Keeper", "keeper"))
            .await
            .expect("upsert");
        upsert_npc(pool, &sample_npc("npc_2", "Stayer", "stayer"))
            .await
            .expect("upsert");

        delete_npc_by_id(pool, "npc_1").await.expect("delete");

        assert!(
            find_npc_by_id(pool, "npc_1")
                .await
                .expect("query")
                .is_none()
        );
        assert!(
            find_npc_by_id(pool, "npc_2")
                .await
                .expect("query")
                .is_some()
        );
        assert_eq!(list_npcs(pool).await.expect("list").len(), 1);
    }

    #[tokio::test]
    async fn location_find_by_slug_round_trips() {
        let database = temp_db().await;
        let pool = &database.pool;
        upsert_location(
            pool,
            &sample_location("loc_1", "Neverwinter Harbor", "neverwinter-harbor"),
        )
        .await
        .expect("upsert location");

        let found = find_location_by_slug(pool, "neverwinter-harbor")
            .await
            .expect("query")
            .expect("location present");
        assert_eq!(found.id, "loc_1");
        assert_eq!(found.name, "Neverwinter Harbor");
        assert_eq!(found.exports, "smoked eel, river pearls");

        // A non-matching slug returns nothing.
        assert!(
            find_location_by_slug(pool, "missing")
                .await
                .expect("query")
                .is_none()
        );
    }

    // The 6 non-npc tables are generated by `impl_entity_table!` from a single
    // column list, so bind / SELECT / row_to order can't diverge. A round-trip per
    // table still guards the things the macro can't: a mistyped column name (would
    // be a runtime SQL error) and the `opt` columns (`liege`/`loyalty_type`/
    // `rank_custom`). A forgotten column is already a compile error (struct-literal
    // completeness).

    #[tokio::test]
    async fn faction_round_trips_including_optional_liege_and_loyalty() {
        let database = temp_db().await;
        let pool = &database.pool;
        // A houses vassal exercises the nullable `liege`/`loyalty_type` columns with
        // values (a non-houses faction stores them NULL -> None).
        let faction = FactionRow {
            id: "fac_1".to_string(),
            slug: "house-corvane".to_string(),
            name: "House Corvane".to_string(),
            vault_path: "factions/houses/House Corvane.md".to_string(),
            kind_type: "major_vassal".to_string(),
            category: "houses".to_string(),
            public_description: "A loyal banner house.".to_string(),
            reputation: "respected".to_string(),
            symbol_description: "A black raven on grey.".to_string(),
            want: "Reclaim the river tariffs.".to_string(),
            obstacle: "Their liege favors a rival house.".to_string(),
            action: "Quiet bribes and a marriage pact.".to_string(),
            consequence: "An open feud on the docks.".to_string(),
            leader: "Lord Aldous Corvane".to_string(),
            sphere_of_influence: "The lower river wards".to_string(),
            resources_assets: "[\"river tolls\", \"a mercenary charter\"]".to_string(),
            allies: "[\"Dust Choir\"]".to_string(),
            rivals_enemies: "[\"House Vey\"]".to_string(),
            liege: Some("House Vaurel".to_string()),
            loyalty_type: Some("oath".to_string()),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T00:00:00Z".to_string(),
        };
        upsert_faction(pool, &faction).await.expect("upsert");

        let found = find_faction_by_id(pool, "fac_1")
            .await
            .expect("query")
            .expect("present");
        assert_eq!(found.name, "House Corvane");
        assert_eq!(found.category, "houses");
        assert_eq!(found.symbol_description, "A black raven on grey.");
        assert_eq!(found.want, "Reclaim the river tariffs.");
        assert_eq!(found.consequence, "An open feud on the docks.");
        // The nullable houses-only columns round-trip their values.
        assert_eq!(found.liege, Some("House Vaurel".to_string()));
        assert_eq!(found.loyalty_type, Some("oath".to_string()));
        // by_slug + by_name_or_slug share the macro's column list.
        assert_eq!(
            find_faction_by_slug(pool, "house-corvane")
                .await
                .expect("query")
                .map(|f| f.id),
            Some("fac_1".to_string())
        );
    }

    #[tokio::test]
    async fn transaction_commit_persists_and_rollback_discards() {
        let database = temp_db().await;

        // A committed transaction persists the row — exercising the executor-generic
        // upsert over a `&mut Transaction` that the atomic save/reap paths rely on
        // (P6.1).
        let mut tx = database.begin().await.expect("begin");
        upsert_npc(&mut *tx, &sample_npc("npc_commit", "Mara", "mara"))
            .await
            .expect("upsert in tx");
        tx.commit().await.expect("commit");
        assert!(
            find_npc_by_id(&database.pool, "npc_commit")
                .await
                .expect("query")
                .is_some()
        );

        // A dropped (un-committed) transaction rolls back — nothing persists, so a
        // mid-sequence failure can't leave a half-written projection behind.
        let mut tx = database.begin().await.expect("begin");
        upsert_npc(&mut *tx, &sample_npc("npc_rollback", "Vex", "vex"))
            .await
            .expect("upsert in tx");
        drop(tx);
        assert!(
            find_npc_by_id(&database.pool, "npc_rollback")
                .await
                .expect("query")
                .is_none()
        );
    }

    #[tokio::test]
    async fn god_round_trips_including_optional_rank_custom() {
        let database = temp_db().await;
        let pool = &database.pool;
        let god = GodRow {
            id: "god_1".to_string(),
            slug: "the-tidemother".to_string(),
            name: "The Tidemother".to_string(),
            vault_path: "gods/the-tidemother.md".to_string(),
            epithet: "She Who Drowns".to_string(),
            rank: "other".to_string(),
            rank_custom: Some("primordial".to_string()),
            alignment: "CN".to_string(),
            domains: "[\"sea\",\"storms\"]".to_string(),
            symbol: "A cresting wave".to_string(),
            appearance: "A woman of green glass.".to_string(),
            dogma: "Give the sea its due.".to_string(),
            realm: "The Drowned Hall".to_string(),
            worshippers: "sailors, smugglers".to_string(),
            clergy: "The Salt Wardens".to_string(),
            allies: "[\"Storm Lord\"]".to_string(),
            rivals: "[\"The Sun King\"]".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T00:00:00Z".to_string(),
        };
        upsert_god(pool, &god).await.expect("upsert");

        let found = find_god_by_id(pool, "god_1")
            .await
            .expect("query")
            .expect("present");
        assert_eq!(found.epithet, "She Who Drowns");
        assert_eq!(found.rank_custom, Some("primordial".to_string()));
        assert_eq!(found.rivals, "[\"The Sun King\"]");
    }

    #[tokio::test]
    async fn item_round_trips_every_column() {
        let database = temp_db().await;
        let pool = &database.pool;
        let item = ItemRow {
            id: "item_1".to_string(),
            slug: "sunfang".to_string(),
            name: "Sunfang".to_string(),
            vault_path: "items/sunfang.md".to_string(),
            category: "weapon".to_string(),
            rarity: "rare".to_string(),
            attunement: "required".to_string(),
            materials: "[\"meteoric iron\"]".to_string(),
            appearance: "A blade that holds the dawn.".to_string(),
            abilities: "Sheds sunlight on command.".to_string(),
            drawbacks: "Cold to undead touch.".to_string(),
            history: "Forged for a fallen paladin.".to_string(),
            value: "4500 gp".to_string(),
            location: "Vault of Embers".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T00:00:00Z".to_string(),
        };
        upsert_item(pool, &item).await.expect("upsert");

        let found = find_item_by_id(pool, "item_1")
            .await
            .expect("query")
            .expect("present");
        assert_eq!(found.category, "weapon");
        assert_eq!(found.materials, "[\"meteoric iron\"]");
        assert_eq!(found.location, "Vault of Embers");
    }

    #[tokio::test]
    async fn dungeon_round_trips_beats_and_dials() {
        let database = temp_db().await;
        let pool = &database.pool;
        let dungeon = DungeonRow {
            id: "dun_1".to_string(),
            slug: "the-sunken-crypt".to_string(),
            name: "The Sunken Crypt".to_string(),
            vault_path: "dungeons/the-sunken-crypt.md".to_string(),
            location: "Under the harbor".to_string(),
            story: "A drowned king stirs.".to_string(),
            premise: "Recover the tide-pearl.".to_string(),
            topology: "linear".to_string(),
            tone: "tragedy".to_string(),
            twist: "ally".to_string(),
            beats_json: "[{\"name\":\"Entrance\"}]".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T00:00:00Z".to_string(),
        };
        upsert_dungeon(pool, &dungeon).await.expect("upsert");

        let found = find_dungeon_by_id(pool, "dun_1")
            .await
            .expect("query")
            .expect("present");
        assert_eq!(found.topology, "linear");
        assert_eq!(found.beats_json, "[{\"name\":\"Entrance\"}]");
        assert_eq!(found.story, "A drowned king stirs.");
    }

    #[tokio::test]
    async fn event_round_trips_body() {
        let database = temp_db().await;
        let pool = &database.pool;
        let event = EventRow {
            id: "evt_1".to_string(),
            slug: "the-long-night".to_string(),
            name: "The Long Night".to_string(),
            vault_path: "events/the-long-night.md".to_string(),
            body: "The sun failed to rise for a tenday.".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T00:00:00Z".to_string(),
        };
        upsert_event(pool, &event).await.expect("upsert");

        let found = find_event_by_name_or_slug(pool, "the long night")
            .await
            .expect("query")
            .expect("present");
        assert_eq!(found.id, "evt_1");
        assert_eq!(found.body, "The sun failed to rise for a tenday.");
    }

    #[tokio::test]
    async fn monster_round_trips_and_searches() {
        // Validates the 0021 migration column names against the generated CRUD
        // (a typo would be a runtime SQL error) and the REAL `cr_sort` column.
        let database = temp_db().await;
        let pool = &database.pool;
        let monster = MonsterRow {
            id: "goblin-warrior".to_string(),
            slug: "goblin-warrior".to_string(),
            name: "Goblin Warrior".to_string(),
            cr: "1/4 (XP 50; PB +2)".to_string(),
            cr_sort: 0.25,
            creature_type: "Fey (Goblinoid)".to_string(),
            size: "Small".to_string(),
            source: "XMM".to_string(),
            created_at: "2026-06-20T00:00:00Z".to_string(),
            updated_at: "2026-06-20T00:00:00Z".to_string(),
        };
        upsert_monster(pool, &monster).await.expect("upsert");

        let found = find_monster_by_slug(pool, "goblin-warrior")
            .await
            .expect("query")
            .expect("present");
        assert_eq!(found.name, "Goblin Warrior");
        assert_eq!(found.cr_sort, 0.25);
        assert_eq!(found.creature_type, "Fey (Goblinoid)");

        // The LIKE typeahead + count + clear used by the import path.
        let hits = search_monsters_by_name(pool, "gob", 6)
            .await
            .expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(count_monsters(pool).await.expect("count"), 1);
        clear_monsters(pool).await.expect("clear");
        assert_eq!(count_monsters(pool).await.expect("count"), 0);
    }

    #[tokio::test]
    async fn search_escapes_like_wildcards_in_the_query() {
        let database = temp_db().await;
        let pool = &database.pool;
        // A name containing no wildcard chars that an *unescaped* `%`/`_` query
        // would wrongly match.
        upsert_npc(pool, &sample_npc("npc_1", "505 Regiment", "regiment-505"))
            .await
            .expect("upsert");

        // `50%` must be treated literally (escaped), so it does NOT match "505...".
        assert!(
            search_npcs_by_name(pool, "50%", 10)
                .await
                .expect("search")
                .is_empty(),
            "`%` in the query should be escaped, not act as a wildcard"
        );
        // A literal substring still matches.
        assert_eq!(
            search_npcs_by_name(pool, "505", 10)
                .await
                .expect("search")
                .len(),
            1
        );
    }

    #[test]
    fn like_contains_escapes_sql_wildcards() {
        assert_eq!(crate::db_macros::like_contains("50%"), "%50\\%%");
        assert_eq!(crate::db_macros::like_contains("a_b"), "%a\\_b%");
        assert_eq!(crate::db_macros::like_contains("c\\d"), "%c\\\\d%");
        // Trims + lowercases like the old inline pattern did.
        assert_eq!(crate::db_macros::like_contains("  RAVEN "), "%raven%");
    }

    #[tokio::test]
    async fn same_named_rows_resolve_deterministically_by_id() {
        let database = temp_db().await;
        let pool = &database.pool;
        // Two npcs share a (lowercased) name but differ by id/slug/path.
        let mut raven_b = sample_npc("npc_b", "Raven", "raven-2");
        raven_b.vault_path = "npcs/raven-2.md".to_string();
        let mut raven_a = sample_npc("npc_a", "Raven", "raven-1");
        raven_a.vault_path = "npcs/raven-1.md".to_string();
        upsert_npc(pool, &raven_b).await.expect("upsert");
        upsert_npc(pool, &raven_a).await.expect("upsert");

        // The `, id ASC` tie-break makes the name resolution deterministic.
        assert_eq!(
            find_npc_by_name_or_slug(pool, "Raven")
                .await
                .expect("query")
                .map(|n| n.id),
            Some("npc_a".to_string())
        );
        // search is ordered by (name, id), so the lower id comes first.
        let hits = search_npcs_by_name(pool, "Raven", 10)
            .await
            .expect("search");
        assert_eq!(
            hits.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            vec!["npc_a", "npc_b"]
        );
    }
}
