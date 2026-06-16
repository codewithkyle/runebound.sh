use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use sqlx::migrate::Migrator;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Row, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
const APP_DIR_NAME: &str = "runebound.sh";

pub struct Database {
    pub pool: SqlitePool,
    pub path: PathBuf,
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
    pub kind_custom: Option<String>,
    pub public_description: String,
    pub true_agenda: String,
    pub methods: String,
    pub leadership: String,
    pub headquarters: String,
    pub sphere_of_influence: String,
    pub resources_assets: String,
    pub allies: String,
    pub rivals_enemies: String,
    pub reputation: String,
    pub current_tension: String,
    pub goals_short_term: String,
    pub goals_long_term: String,
    pub symbol_description: String,
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

pub async fn search_npcs_by_name(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> Result<Vec<NpcRow>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let rows = sqlx::query(
        "SELECT id, slug, name, race, occupation, sex, age, height, weight_lbs, background, want_need, secret_obstacle, carrying, location, vault_path, created_at, updated_at
         FROM npcs
         WHERE lower(name) LIKE ?1
         ORDER BY name COLLATE NOCASE ASC
         LIMIT ?2",
    )
    .bind(pattern)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to search npcs by name")?;

    rows.into_iter().map(row_to_npc).collect()
}

pub async fn search_locations_by_name(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> Result<Vec<LocationRow>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let rows = sqlx::query(
        "SELECT id, slug, name, vault_path, kind_type, kind_custom, visual_description, history_background, exports, tone, authority, danger_level, current_tension, created_at, updated_at
         FROM locations
         WHERE lower(name) LIKE ?1
         ORDER BY name COLLATE NOCASE ASC
         LIMIT ?2",
    )
    .bind(pattern)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to search locations by name")?;

    rows.into_iter().map(row_to_location).collect()
}

pub async fn search_factions_by_name(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> Result<Vec<FactionRow>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let rows = sqlx::query(
        "SELECT id, slug, name, vault_path, kind_type, kind_custom, public_description, true_agenda, methods, leadership, headquarters, sphere_of_influence, resources_assets, allies, rivals_enemies, reputation, current_tension, goals_short_term, goals_long_term, symbol_description, created_at, updated_at
         FROM factions
         WHERE lower(name) LIKE ?1
         ORDER BY name COLLATE NOCASE ASC
         LIMIT ?2",
    )
    .bind(pattern)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to search factions by name")?;

    rows.into_iter().map(row_to_faction).collect()
}

pub async fn search_items_by_name(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> Result<Vec<ItemRow>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let rows = sqlx::query(
        "SELECT id, slug, name, vault_path, category, rarity, attunement, materials, appearance, abilities, drawbacks, history, value, location, created_at, updated_at
         FROM items
         WHERE lower(name) LIKE ?1
         ORDER BY name COLLATE NOCASE ASC
         LIMIT ?2",
    )
    .bind(pattern)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to search items by name")?;

    rows.into_iter().map(row_to_item).collect()
}

pub async fn find_npc_by_name_or_slug(pool: &SqlitePool, input: &str) -> Result<Option<NpcRow>> {
    let normalized = input.trim().to_ascii_lowercase();
    let row = sqlx::query(
        "SELECT id, slug, name, race, occupation, sex, age, height, weight_lbs, background, want_need, secret_obstacle, carrying, location, vault_path, created_at, updated_at
         FROM npcs
         WHERE lower(name) = ?1 OR lower(slug) = ?2
         ORDER BY CASE WHEN lower(name) = ?1 THEN 0 ELSE 1 END
         LIMIT 1",
    )
    .bind(&normalized)
    .bind(&normalized)
    .fetch_optional(pool)
    .await
    .context("failed to find npc by name or slug")?;

    row.map(row_to_npc).transpose()
}

pub async fn list_npcs(pool: &SqlitePool) -> Result<Vec<NpcRow>> {
    let rows = sqlx::query(
        "SELECT id, slug, name, race, occupation, sex, age, height, weight_lbs, background, want_need, secret_obstacle, carrying, location, vault_path, created_at, updated_at
         FROM npcs",
    )
    .fetch_all(pool)
    .await
    .context("failed to list npcs")?;

    rows.into_iter().map(row_to_npc).collect()
}

pub async fn find_location_by_name_or_slug(
    pool: &SqlitePool,
    input: &str,
) -> Result<Option<LocationRow>> {
    let normalized = input.trim().to_ascii_lowercase();
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, kind_type, kind_custom, visual_description, history_background, exports, tone, authority, danger_level, current_tension, created_at, updated_at
         FROM locations
         WHERE lower(name) = ?1 OR lower(slug) = ?2
         ORDER BY CASE WHEN lower(name) = ?1 THEN 0 ELSE 1 END
         LIMIT 1",
    )
    .bind(&normalized)
    .bind(&normalized)
    .fetch_optional(pool)
    .await
    .context("failed to find location by name or slug")?;

    row.map(row_to_location).transpose()
}

pub async fn find_faction_by_name_or_slug(
    pool: &SqlitePool,
    input: &str,
) -> Result<Option<FactionRow>> {
    let normalized = input.trim().to_ascii_lowercase();
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, kind_type, kind_custom, public_description, true_agenda, methods, leadership, headquarters, sphere_of_influence, resources_assets, allies, rivals_enemies, reputation, current_tension, goals_short_term, goals_long_term, symbol_description, created_at, updated_at
         FROM factions
         WHERE lower(name) = ?1 OR lower(slug) = ?2
         ORDER BY CASE WHEN lower(name) = ?1 THEN 0 ELSE 1 END
         LIMIT 1",
    )
    .bind(&normalized)
    .bind(&normalized)
    .fetch_optional(pool)
    .await
    .context("failed to find faction by name or slug")?;

    row.map(row_to_faction).transpose()
}

pub async fn find_item_by_name_or_slug(pool: &SqlitePool, input: &str) -> Result<Option<ItemRow>> {
    let normalized = input.trim().to_ascii_lowercase();
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, category, rarity, attunement, materials, appearance, abilities, drawbacks, history, value, location, created_at, updated_at
         FROM items
         WHERE lower(name) = ?1 OR lower(slug) = ?2
         ORDER BY CASE WHEN lower(name) = ?1 THEN 0 ELSE 1 END
         LIMIT 1",
    )
    .bind(&normalized)
    .bind(&normalized)
    .fetch_optional(pool)
    .await
    .context("failed to find item by name or slug")?;

    row.map(row_to_item).transpose()
}

pub async fn list_locations(pool: &SqlitePool) -> Result<Vec<LocationRow>> {
    let rows = sqlx::query(
        "SELECT id, slug, name, vault_path, kind_type, kind_custom, visual_description, history_background, exports, tone, authority, danger_level, current_tension, created_at, updated_at
         FROM locations",
    )
    .fetch_all(pool)
    .await
    .context("failed to list locations")?;

    rows.into_iter().map(row_to_location).collect()
}

pub async fn list_factions(pool: &SqlitePool) -> Result<Vec<FactionRow>> {
    let rows = sqlx::query(
        "SELECT id, slug, name, vault_path, kind_type, kind_custom, public_description, true_agenda, methods, leadership, headquarters, sphere_of_influence, resources_assets, allies, rivals_enemies, reputation, current_tension, goals_short_term, goals_long_term, symbol_description, created_at, updated_at
         FROM factions",
    )
    .fetch_all(pool)
    .await
    .context("failed to list factions")?;

    rows.into_iter().map(row_to_faction).collect()
}

pub async fn list_items(pool: &SqlitePool) -> Result<Vec<ItemRow>> {
    let rows = sqlx::query(
        "SELECT id, slug, name, vault_path, category, rarity, attunement, materials, appearance, abilities, drawbacks, history, value, location, created_at, updated_at
         FROM items",
    )
    .fetch_all(pool)
    .await
    .context("failed to list items")?;

    rows.into_iter().map(row_to_item).collect()
}

pub async fn search_gods_by_name(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> Result<Vec<GodRow>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let rows = sqlx::query(
        "SELECT id, slug, name, vault_path, epithet, rank, rank_custom, alignment, domains, symbol, appearance, dogma, realm, worshippers, clergy, allies, rivals, created_at, updated_at
         FROM gods
         WHERE lower(name) LIKE ?1
         ORDER BY name COLLATE NOCASE ASC
         LIMIT ?2",
    )
    .bind(pattern)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to search gods by name")?;

    rows.into_iter().map(row_to_god).collect()
}

pub async fn find_god_by_name_or_slug(pool: &SqlitePool, input: &str) -> Result<Option<GodRow>> {
    let normalized = input.trim().to_ascii_lowercase();
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, epithet, rank, rank_custom, alignment, domains, symbol, appearance, dogma, realm, worshippers, clergy, allies, rivals, created_at, updated_at
         FROM gods
         WHERE lower(name) = ?1 OR lower(slug) = ?2
         ORDER BY CASE WHEN lower(name) = ?1 THEN 0 ELSE 1 END
         LIMIT 1",
    )
    .bind(&normalized)
    .bind(&normalized)
    .fetch_optional(pool)
    .await
    .context("failed to find god by name or slug")?;

    row.map(row_to_god).transpose()
}

pub async fn find_god_by_slug(pool: &SqlitePool, slug: &str) -> Result<Option<GodRow>> {
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, epithet, rank, rank_custom, alignment, domains, symbol, appearance, dogma, realm, worshippers, clergy, allies, rivals, created_at, updated_at FROM gods WHERE slug = ?1",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .context("failed to query god by slug")?;

    row.map(row_to_god).transpose()
}

pub async fn find_god_by_id(pool: &SqlitePool, id: &str) -> Result<Option<GodRow>> {
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, epithet, rank, rank_custom, alignment, domains, symbol, appearance, dogma, realm, worshippers, clergy, allies, rivals, created_at, updated_at FROM gods WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("failed to query god by id")?;

    row.map(row_to_god).transpose()
}

pub async fn list_gods(pool: &SqlitePool) -> Result<Vec<GodRow>> {
    let rows = sqlx::query(
        "SELECT id, slug, name, vault_path, epithet, rank, rank_custom, alignment, domains, symbol, appearance, dogma, realm, worshippers, clergy, allies, rivals, created_at, updated_at
         FROM gods",
    )
    .fetch_all(pool)
    .await
    .context("failed to list gods")?;

    rows.into_iter().map(row_to_god).collect()
}

pub async fn upsert_god(pool: &SqlitePool, god: &GodRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO gods (id, slug, name, vault_path, epithet, rank, rank_custom, alignment, domains, symbol, appearance, dogma, realm, worshippers, clergy, allies, rivals, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
         ON CONFLICT(id) DO UPDATE SET
            slug = excluded.slug,
            name = excluded.name,
            vault_path = excluded.vault_path,
            epithet = excluded.epithet,
            rank = excluded.rank,
            rank_custom = excluded.rank_custom,
            alignment = excluded.alignment,
            domains = excluded.domains,
            symbol = excluded.symbol,
            appearance = excluded.appearance,
            dogma = excluded.dogma,
            realm = excluded.realm,
            worshippers = excluded.worshippers,
            clergy = excluded.clergy,
            allies = excluded.allies,
            rivals = excluded.rivals,
            updated_at = excluded.updated_at",
    )
    .bind(&god.id)
    .bind(&god.slug)
    .bind(&god.name)
    .bind(&god.vault_path)
    .bind(&god.epithet)
    .bind(&god.rank)
    .bind(&god.rank_custom)
    .bind(&god.alignment)
    .bind(&god.domains)
    .bind(&god.symbol)
    .bind(&god.appearance)
    .bind(&god.dogma)
    .bind(&god.realm)
    .bind(&god.worshippers)
    .bind(&god.clergy)
    .bind(&god.allies)
    .bind(&god.rivals)
    .bind(&god.created_at)
    .bind(&god.updated_at)
    .execute(pool)
    .await
    .context("failed to upsert god")?;

    Ok(())
}

pub async fn delete_god_by_id(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM gods WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .context("failed to delete god row")?;

    Ok(())
}

pub async fn search_dungeons_by_name(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> Result<Vec<DungeonRow>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let rows = sqlx::query(
        "SELECT id, slug, name, vault_path, location, premise, topology, tone, twist, beats_json, created_at, updated_at
         FROM dungeons
         WHERE lower(name) LIKE ?1
         ORDER BY name COLLATE NOCASE ASC
         LIMIT ?2",
    )
    .bind(pattern)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to search dungeons by name")?;

    rows.into_iter().map(row_to_dungeon).collect()
}

pub async fn find_dungeon_by_name_or_slug(
    pool: &SqlitePool,
    input: &str,
) -> Result<Option<DungeonRow>> {
    let normalized = input.trim().to_ascii_lowercase();
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, location, premise, topology, tone, twist, beats_json, created_at, updated_at
         FROM dungeons
         WHERE lower(name) = ?1 OR lower(slug) = ?2
         ORDER BY CASE WHEN lower(name) = ?1 THEN 0 ELSE 1 END
         LIMIT 1",
    )
    .bind(&normalized)
    .bind(&normalized)
    .fetch_optional(pool)
    .await
    .context("failed to find dungeon by name or slug")?;

    row.map(row_to_dungeon).transpose()
}

pub async fn find_dungeon_by_slug(pool: &SqlitePool, slug: &str) -> Result<Option<DungeonRow>> {
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, location, premise, topology, tone, twist, beats_json, created_at, updated_at FROM dungeons WHERE slug = ?1",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .context("failed to query dungeon by slug")?;

    row.map(row_to_dungeon).transpose()
}

pub async fn find_dungeon_by_id(pool: &SqlitePool, id: &str) -> Result<Option<DungeonRow>> {
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, location, premise, topology, tone, twist, beats_json, created_at, updated_at FROM dungeons WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("failed to query dungeon by id")?;

    row.map(row_to_dungeon).transpose()
}

pub async fn list_dungeons(pool: &SqlitePool) -> Result<Vec<DungeonRow>> {
    let rows = sqlx::query(
        "SELECT id, slug, name, vault_path, location, premise, topology, tone, twist, beats_json, created_at, updated_at
         FROM dungeons",
    )
    .fetch_all(pool)
    .await
    .context("failed to list dungeons")?;

    rows.into_iter().map(row_to_dungeon).collect()
}

pub async fn upsert_dungeon(pool: &SqlitePool, dungeon: &DungeonRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO dungeons (id, slug, name, vault_path, location, premise, topology, tone, twist, beats_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(id) DO UPDATE SET
            slug = excluded.slug,
            name = excluded.name,
            vault_path = excluded.vault_path,
            location = excluded.location,
            premise = excluded.premise,
            topology = excluded.topology,
            tone = excluded.tone,
            twist = excluded.twist,
            beats_json = excluded.beats_json,
            updated_at = excluded.updated_at",
    )
    .bind(&dungeon.id)
    .bind(&dungeon.slug)
    .bind(&dungeon.name)
    .bind(&dungeon.vault_path)
    .bind(&dungeon.location)
    .bind(&dungeon.premise)
    .bind(&dungeon.topology)
    .bind(&dungeon.tone)
    .bind(&dungeon.twist)
    .bind(&dungeon.beats_json)
    .bind(&dungeon.created_at)
    .bind(&dungeon.updated_at)
    .execute(pool)
    .await
    .context("failed to upsert dungeon")?;

    Ok(())
}

pub async fn delete_dungeon_by_id(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM dungeons WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .context("failed to delete dungeon row")?;

    Ok(())
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

pub async fn find_location_by_slug(pool: &SqlitePool, slug: &str) -> Result<Option<LocationRow>> {
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, kind_type, kind_custom, visual_description, history_background, exports, tone, authority, danger_level, current_tension, created_at, updated_at FROM locations WHERE slug = ?1",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .context("failed to query location by slug")?;

    row.map(row_to_location).transpose()
}

pub async fn find_location_by_id(pool: &SqlitePool, id: &str) -> Result<Option<LocationRow>> {
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, kind_type, kind_custom, visual_description, history_background, exports, tone, authority, danger_level, current_tension, created_at, updated_at FROM locations WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("failed to query location by id")?;

    row.map(row_to_location).transpose()
}

pub async fn find_faction_by_slug(pool: &SqlitePool, slug: &str) -> Result<Option<FactionRow>> {
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, kind_type, kind_custom, public_description, true_agenda, methods, leadership, headquarters, sphere_of_influence, resources_assets, allies, rivals_enemies, reputation, current_tension, goals_short_term, goals_long_term, symbol_description, created_at, updated_at FROM factions WHERE slug = ?1",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .context("failed to query faction by slug")?;

    row.map(row_to_faction).transpose()
}

pub async fn find_faction_by_id(pool: &SqlitePool, id: &str) -> Result<Option<FactionRow>> {
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, kind_type, kind_custom, public_description, true_agenda, methods, leadership, headquarters, sphere_of_influence, resources_assets, allies, rivals_enemies, reputation, current_tension, goals_short_term, goals_long_term, symbol_description, created_at, updated_at FROM factions WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("failed to query faction by id")?;

    row.map(row_to_faction).transpose()
}

pub async fn find_item_by_id(pool: &SqlitePool, id: &str) -> Result<Option<ItemRow>> {
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, category, rarity, attunement, materials, appearance, abilities, drawbacks, history, value, location, created_at, updated_at FROM items WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("failed to query item by id")?;

    row.map(row_to_item).transpose()
}

pub async fn upsert_location(pool: &SqlitePool, location: &LocationRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO locations (id, slug, name, vault_path, kind_type, kind_custom, visual_description, history_background, exports, tone, authority, danger_level, current_tension, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(id) DO UPDATE SET
            slug = excluded.slug,
            name = excluded.name,
            vault_path = excluded.vault_path,
            kind_type = excluded.kind_type,
            kind_custom = excluded.kind_custom,
            visual_description = excluded.visual_description,
            history_background = excluded.history_background,
            exports = excluded.exports,
            tone = excluded.tone,
            authority = excluded.authority,
            danger_level = excluded.danger_level,
            current_tension = excluded.current_tension,
            updated_at = excluded.updated_at",
    )
    .bind(&location.id)
    .bind(&location.slug)
    .bind(&location.name)
    .bind(&location.vault_path)
    .bind(&location.kind_type)
    .bind(&location.kind_custom)
    .bind(&location.visual_description)
    .bind(&location.history_background)
    .bind(&location.exports)
    .bind(&location.tone)
    .bind(&location.authority)
    .bind(&location.danger_level)
    .bind(&location.current_tension)
    .bind(&location.created_at)
    .bind(&location.updated_at)
    .execute(pool)
    .await
    .context("failed to upsert location")?;

    Ok(())
}

pub async fn upsert_faction(pool: &SqlitePool, faction: &FactionRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO factions (id, slug, name, vault_path, kind_type, kind_custom, public_description, true_agenda, methods, leadership, headquarters, sphere_of_influence, resources_assets, allies, rivals_enemies, reputation, current_tension, goals_short_term, goals_long_term, symbol_description, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
         ON CONFLICT(id) DO UPDATE SET
            slug = excluded.slug,
            name = excluded.name,
            vault_path = excluded.vault_path,
            kind_type = excluded.kind_type,
            kind_custom = excluded.kind_custom,
            public_description = excluded.public_description,
            true_agenda = excluded.true_agenda,
            methods = excluded.methods,
            leadership = excluded.leadership,
            headquarters = excluded.headquarters,
            sphere_of_influence = excluded.sphere_of_influence,
            resources_assets = excluded.resources_assets,
            allies = excluded.allies,
            rivals_enemies = excluded.rivals_enemies,
            reputation = excluded.reputation,
            current_tension = excluded.current_tension,
            goals_short_term = excluded.goals_short_term,
            goals_long_term = excluded.goals_long_term,
            symbol_description = excluded.symbol_description,
            updated_at = excluded.updated_at",
    )
    .bind(&faction.id)
    .bind(&faction.slug)
    .bind(&faction.name)
    .bind(&faction.vault_path)
    .bind(&faction.kind_type)
    .bind(&faction.kind_custom)
    .bind(&faction.public_description)
    .bind(&faction.true_agenda)
    .bind(&faction.methods)
    .bind(&faction.leadership)
    .bind(&faction.headquarters)
    .bind(&faction.sphere_of_influence)
    .bind(&faction.resources_assets)
    .bind(&faction.allies)
    .bind(&faction.rivals_enemies)
    .bind(&faction.reputation)
    .bind(&faction.current_tension)
    .bind(&faction.goals_short_term)
    .bind(&faction.goals_long_term)
    .bind(&faction.symbol_description)
    .bind(&faction.created_at)
    .bind(&faction.updated_at)
    .execute(pool)
    .await
    .context("failed to upsert faction")?;

    Ok(())
}

pub async fn find_npc_by_id(pool: &SqlitePool, id: &str) -> Result<Option<NpcRow>> {
    let row = sqlx::query(
        "SELECT id, slug, name, race, occupation, sex, age, height, weight_lbs, background, want_need, secret_obstacle, carrying, location, vault_path, created_at, updated_at FROM npcs WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("failed to query npc by id")?;

    row.map(row_to_npc).transpose()
}

pub async fn upsert_npc(pool: &SqlitePool, npc: &NpcRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO npcs (id, slug, name, race, occupation, sex, age, height, weight_lbs, background, want_need, secret_obstacle, carrying, location, vault_path, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT(id) DO UPDATE SET
            slug = excluded.slug,
            name = excluded.name,
            race = excluded.race,
            occupation = excluded.occupation,
            sex = excluded.sex,
            age = excluded.age,
            height = excluded.height,
            weight_lbs = excluded.weight_lbs,
            background = excluded.background,
            want_need = excluded.want_need,
            secret_obstacle = excluded.secret_obstacle,
            carrying = excluded.carrying,
            location = excluded.location,
            vault_path = excluded.vault_path,
            updated_at = excluded.updated_at",
    )
    .bind(&npc.id)
    .bind(&npc.slug)
    .bind(&npc.name)
    .bind(&npc.race)
    .bind(&npc.occupation)
    .bind(&npc.sex)
    .bind(&npc.age)
    .bind(&npc.height)
    .bind(&npc.weight_lbs)
    .bind(&npc.background)
    .bind(&npc.want_need)
    .bind(&npc.secret_obstacle)
    .bind(&npc.carrying)
    .bind(&npc.location)
    .bind(&npc.vault_path)
    .bind(&npc.created_at)
    .bind(&npc.updated_at)
    .execute(pool)
    .await
    .context("failed to upsert npc")?;

    Ok(())
}

pub async fn upsert_item(pool: &SqlitePool, item: &ItemRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO items (id, slug, name, vault_path, category, rarity, attunement, materials, appearance, abilities, drawbacks, history, value, location, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(id) DO UPDATE SET
            slug = excluded.slug,
            name = excluded.name,
            vault_path = excluded.vault_path,
            category = excluded.category,
            rarity = excluded.rarity,
            attunement = excluded.attunement,
            materials = excluded.materials,
            appearance = excluded.appearance,
            abilities = excluded.abilities,
            drawbacks = excluded.drawbacks,
            history = excluded.history,
            value = excluded.value,
            location = excluded.location,
            updated_at = excluded.updated_at",
    )
    .bind(&item.id)
    .bind(&item.slug)
    .bind(&item.name)
    .bind(&item.vault_path)
    .bind(&item.category)
    .bind(&item.rarity)
    .bind(&item.attunement)
    .bind(&item.materials)
    .bind(&item.appearance)
    .bind(&item.abilities)
    .bind(&item.drawbacks)
    .bind(&item.history)
    .bind(&item.value)
    .bind(&item.location)
    .bind(&item.created_at)
    .bind(&item.updated_at)
    .execute(pool)
    .await
    .context("failed to upsert item")?;

    Ok(())
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

pub async fn upsert_document_index(
    pool: &SqlitePool,
    doc_type: &str,
    slug: &str,
    title: Option<&str>,
    vault_path: &str,
    created_at: &str,
    updated_at: &str,
) -> Result<()> {
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
    .execute(pool)
    .await
    .context("failed to upsert documents index")?;

    Ok(())
}

pub async fn delete_npc_by_id(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM npcs WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .context("failed to delete npc row")?;

    Ok(())
}

pub async fn delete_location_by_id(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM locations WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .context("failed to delete location row")?;

    Ok(())
}

pub async fn delete_faction_by_id(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM factions WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .context("failed to delete faction row")?;

    Ok(())
}

pub async fn delete_item_by_id(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM items WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .context("failed to delete item row")?;

    Ok(())
}

pub async fn search_events_by_name(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> Result<Vec<EventRow>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let rows = sqlx::query(
        "SELECT id, slug, name, vault_path, body, created_at, updated_at
         FROM events
         WHERE lower(name) LIKE ?1
         ORDER BY name COLLATE NOCASE ASC
         LIMIT ?2",
    )
    .bind(pattern)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to search events by name")?;

    rows.into_iter().map(row_to_event).collect()
}

pub async fn find_event_by_name_or_slug(pool: &SqlitePool, input: &str) -> Result<Option<EventRow>> {
    let normalized = input.trim().to_ascii_lowercase();
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, body, created_at, updated_at
         FROM events
         WHERE lower(name) = ?1 OR lower(slug) = ?2
         ORDER BY CASE WHEN lower(name) = ?1 THEN 0 ELSE 1 END
         LIMIT 1",
    )
    .bind(&normalized)
    .bind(&normalized)
    .fetch_optional(pool)
    .await
    .context("failed to find event by name or slug")?;

    row.map(row_to_event).transpose()
}

pub async fn find_event_by_id(pool: &SqlitePool, id: &str) -> Result<Option<EventRow>> {
    let row = sqlx::query(
        "SELECT id, slug, name, vault_path, body, created_at, updated_at FROM events WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("failed to query event by id")?;

    row.map(row_to_event).transpose()
}

pub async fn list_events(pool: &SqlitePool) -> Result<Vec<EventRow>> {
    let rows = sqlx::query(
        "SELECT id, slug, name, vault_path, body, created_at, updated_at
         FROM events",
    )
    .fetch_all(pool)
    .await
    .context("failed to list events")?;

    rows.into_iter().map(row_to_event).collect()
}

pub async fn upsert_event(pool: &SqlitePool, event: &EventRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO events (id, slug, name, vault_path, body, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            slug = excluded.slug,
            name = excluded.name,
            vault_path = excluded.vault_path,
            body = excluded.body,
            updated_at = excluded.updated_at",
    )
    .bind(&event.id)
    .bind(&event.slug)
    .bind(&event.name)
    .bind(&event.vault_path)
    .bind(&event.body)
    .bind(&event.created_at)
    .bind(&event.updated_at)
    .execute(pool)
    .await
    .context("failed to upsert event")?;

    Ok(())
}

pub async fn delete_event_by_id(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM events WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .context("failed to delete event row")?;

    Ok(())
}

pub async fn delete_document_by_vault_path(pool: &SqlitePool, vault_path: &str) -> Result<()> {
    sqlx::query("DELETE FROM documents WHERE vault_path = ?1")
        .bind(vault_path)
        .execute(pool)
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
    let result =
        sqlx::query("UPDATE soft_deletes SET undone_at = ?1 WHERE operation = 'publish' AND undone_at IS NULL")
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

fn row_to_location(row: sqlx::sqlite::SqliteRow) -> Result<LocationRow> {
    Ok(LocationRow {
        id: row.try_get("id").context("locations.id missing")?,
        slug: row.try_get("slug").context("locations.slug missing")?,
        name: row.try_get("name").context("locations.name missing")?,
        vault_path: row
            .try_get("vault_path")
            .context("locations.vault_path missing")?,
        kind_type: row
            .try_get("kind_type")
            .unwrap_or_else(|_| "other".to_string()),
        kind_custom: row.try_get("kind_custom").ok(),
        visual_description: row
            .try_get("visual_description")
            .unwrap_or_else(|_| "Unknown".to_string()),
        history_background: row
            .try_get("history_background")
            .unwrap_or_else(|_| "Unknown".to_string()),
        exports: row
            .try_get("exports")
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        tone: row
            .try_get("tone")
            .unwrap_or_else(|_| "Unknown".to_string()),
        authority: row
            .try_get("authority")
            .unwrap_or_else(|_| "Unknown".to_string()),
        danger_level: row
            .try_get("danger_level")
            .unwrap_or_else(|_| "Unknown".to_string()),
        current_tension: row
            .try_get("current_tension")
            .unwrap_or_else(|_| "Unknown".to_string()),
        created_at: row
            .try_get("created_at")
            .context("locations.created_at missing")?,
        updated_at: row
            .try_get("updated_at")
            .context("locations.updated_at missing")?,
    })
}

fn row_to_faction(row: sqlx::sqlite::SqliteRow) -> Result<FactionRow> {
    Ok(FactionRow {
        id: row.try_get("id").context("factions.id missing")?,
        slug: row.try_get("slug").context("factions.slug missing")?,
        name: row.try_get("name").context("factions.name missing")?,
        vault_path: row
            .try_get("vault_path")
            .context("factions.vault_path missing")?,
        kind_type: row
            .try_get("kind_type")
            .unwrap_or_else(|_| "other".to_string()),
        kind_custom: row.try_get("kind_custom").ok(),
        public_description: row
            .try_get("public_description")
            .unwrap_or_else(|_| "Unknown".to_string()),
        true_agenda: row
            .try_get("true_agenda")
            .unwrap_or_else(|_| "Unknown".to_string()),
        methods: row
            .try_get("methods")
            .unwrap_or_else(|_| "Unknown".to_string()),
        leadership: row
            .try_get("leadership")
            .unwrap_or_else(|_| "Unknown".to_string()),
        headquarters: row
            .try_get("headquarters")
            .unwrap_or_else(|_| "Unknown".to_string()),
        sphere_of_influence: row
            .try_get("sphere_of_influence")
            .unwrap_or_else(|_| "Unknown".to_string()),
        resources_assets: row
            .try_get("resources_assets")
            .unwrap_or_else(|_| "Unknown".to_string()),
        allies: row
            .try_get("allies")
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        rivals_enemies: row
            .try_get("rivals_enemies")
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        reputation: row
            .try_get("reputation")
            .unwrap_or_else(|_| "Unknown".to_string()),
        current_tension: row
            .try_get("current_tension")
            .unwrap_or_else(|_| "Unknown".to_string()),
        goals_short_term: row
            .try_get("goals_short_term")
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        goals_long_term: row
            .try_get("goals_long_term")
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        symbol_description: row
            .try_get("symbol_description")
            .unwrap_or_else(|_| "Unknown".to_string()),
        created_at: row
            .try_get("created_at")
            .context("factions.created_at missing")?,
        updated_at: row
            .try_get("updated_at")
            .context("factions.updated_at missing")?,
    })
}

fn row_to_god(row: sqlx::sqlite::SqliteRow) -> Result<GodRow> {
    Ok(GodRow {
        id: row.try_get("id").context("gods.id missing")?,
        slug: row.try_get("slug").context("gods.slug missing")?,
        name: row.try_get("name").context("gods.name missing")?,
        vault_path: row
            .try_get("vault_path")
            .context("gods.vault_path missing")?,
        epithet: row
            .try_get("epithet")
            .unwrap_or_else(|_| "Unknown".to_string()),
        rank: row.try_get("rank").unwrap_or_else(|_| "other".to_string()),
        rank_custom: row.try_get("rank_custom").ok(),
        alignment: row
            .try_get("alignment")
            .unwrap_or_else(|_| "TN".to_string()),
        domains: row
            .try_get("domains")
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        symbol: row
            .try_get("symbol")
            .unwrap_or_else(|_| "Unknown".to_string()),
        appearance: row
            .try_get("appearance")
            .unwrap_or_else(|_| "Unknown".to_string()),
        dogma: row
            .try_get("dogma")
            .unwrap_or_else(|_| "Unknown".to_string()),
        realm: row
            .try_get("realm")
            .unwrap_or_else(|_| "Unknown".to_string()),
        worshippers: row
            .try_get("worshippers")
            .unwrap_or_else(|_| "Unknown".to_string()),
        clergy: row
            .try_get("clergy")
            .unwrap_or_else(|_| "Unknown".to_string()),
        allies: row
            .try_get("allies")
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        rivals: row
            .try_get("rivals")
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        created_at: row
            .try_get("created_at")
            .context("gods.created_at missing")?,
        updated_at: row
            .try_get("updated_at")
            .context("gods.updated_at missing")?,
    })
}

fn row_to_dungeon(row: sqlx::sqlite::SqliteRow) -> Result<DungeonRow> {
    Ok(DungeonRow {
        id: row.try_get("id").context("dungeons.id missing")?,
        slug: row.try_get("slug").context("dungeons.slug missing")?,
        name: row.try_get("name").context("dungeons.name missing")?,
        vault_path: row
            .try_get("vault_path")
            .context("dungeons.vault_path missing")?,
        location: row.try_get("location").unwrap_or_default(),
        premise: row
            .try_get("premise")
            .unwrap_or_else(|_| "Unknown".to_string()),
        topology: row
            .try_get("topology")
            .unwrap_or_else(|_| "none".to_string()),
        tone: row
            .try_get("tone")
            .unwrap_or_else(|_| "tragedy".to_string()),
        twist: row
            .try_get("twist")
            .unwrap_or_else(|_| "neither".to_string()),
        beats_json: row
            .try_get("beats_json")
            .unwrap_or_else(|_| "[]".to_string()),
        created_at: row
            .try_get("created_at")
            .context("dungeons.created_at missing")?,
        updated_at: row
            .try_get("updated_at")
            .context("dungeons.updated_at missing")?,
    })
}

fn row_to_npc(row: sqlx::sqlite::SqliteRow) -> Result<NpcRow> {
    Ok(NpcRow {
        id: row.try_get("id").context("npcs.id missing")?,
        slug: row.try_get("slug").context("npcs.slug missing")?,
        name: row.try_get("name").context("npcs.name missing")?,
        race: row.try_get("race").context("npcs.race missing")?,
        occupation: row
            .try_get("occupation")
            .unwrap_or_else(|_| "Unknown".to_string()),
        sex: row.try_get("sex").context("npcs.sex missing")?,
        age: row.try_get("age").unwrap_or_else(|_| "Unknown".to_string()),
        height: row
            .try_get("height")
            .unwrap_or_else(|_| "Unknown".to_string()),
        weight_lbs: row
            .try_get("weight_lbs")
            .unwrap_or_else(|_| "Unknown".to_string()),
        background: row
            .try_get("background")
            .unwrap_or_else(|_| "Unknown".to_string()),
        want_need: row
            .try_get("want_need")
            .unwrap_or_else(|_| "Unknown".to_string()),
        secret_obstacle: row
            .try_get("secret_obstacle")
            .unwrap_or_else(|_| "Unknown".to_string()),
        carrying: row
            .try_get("carrying")
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        location: row.try_get("location").context("npcs.location missing")?,
        vault_path: row
            .try_get("vault_path")
            .context("npcs.vault_path missing")?,
        created_at: row
            .try_get("created_at")
            .context("npcs.created_at missing")?,
        updated_at: row
            .try_get("updated_at")
            .context("npcs.updated_at missing")?,
    })
}

fn row_to_item(row: sqlx::sqlite::SqliteRow) -> Result<ItemRow> {
    Ok(ItemRow {
        id: row.try_get("id").context("items.id missing")?,
        slug: row.try_get("slug").context("items.slug missing")?,
        name: row.try_get("name").context("items.name missing")?,
        vault_path: row
            .try_get("vault_path")
            .context("items.vault_path missing")?,
        category: row
            .try_get("category")
            .unwrap_or_else(|_| "other".to_string()),
        rarity: row
            .try_get("rarity")
            .unwrap_or_else(|_| "unknown".to_string()),
        attunement: row
            .try_get("attunement")
            .unwrap_or_else(|_| "Unknown".to_string()),
        materials: row
            .try_get("materials")
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        appearance: row
            .try_get("appearance")
            .unwrap_or_else(|_| "Unknown".to_string()),
        abilities: row
            .try_get("abilities")
            .unwrap_or_else(|_| "Unknown".to_string()),
        drawbacks: row
            .try_get("drawbacks")
            .unwrap_or_else(|_| "Unknown".to_string()),
        history: row
            .try_get("history")
            .unwrap_or_else(|_| "Unknown".to_string()),
        value: row
            .try_get("value")
            .unwrap_or_else(|_| "Unknown".to_string()),
        location: row
            .try_get("location")
            .unwrap_or_else(|_| "Unknown".to_string()),
        created_at: row
            .try_get("created_at")
            .context("items.created_at missing")?,
        updated_at: row
            .try_get("updated_at")
            .context("items.updated_at missing")?,
    })
}

fn row_to_event(row: sqlx::sqlite::SqliteRow) -> Result<EventRow> {
    Ok(EventRow {
        id: row.try_get("id").context("events.id missing")?,
        slug: row.try_get("slug").context("events.slug missing")?,
        name: row.try_get("name").context("events.name missing")?,
        vault_path: row
            .try_get("vault_path")
            .context("events.vault_path missing")?,
        // The body is the whole record, so a missing column is a hard error
        // rather than a defaulted placeholder.
        body: row.try_get("body").context("events.body missing")?,
        created_at: row
            .try_get("created_at")
            .context("events.created_at missing")?,
        updated_at: row
            .try_get("updated_at")
            .context("events.updated_at missing")?,
    })
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
        let path = std::env::temp_dir().join(format!(
            "dnd_db_test_{}_{}.sqlite",
            std::process::id(),
            n
        ));
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
        upsert_npc(pool, &sample_npc("npc_1", "Bram Stoneford", "bram-stoneford"))
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
                &sample_npc(&format!("npc_{i}"), &format!("Guard {i}"), &format!("guard-{i}")),
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

        assert!(find_npc_by_id(pool, "npc_1").await.expect("query").is_none());
        assert!(find_npc_by_id(pool, "npc_2").await.expect("query").is_some());
        assert_eq!(list_npcs(pool).await.expect("list").len(), 1);
    }

    #[tokio::test]
    async fn location_find_by_slug_round_trips() {
        let database = temp_db().await;
        let pool = &database.pool;
        upsert_location(pool, &sample_location("loc_1", "Neverwinter Harbor", "neverwinter-harbor"))
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
        assert!(find_location_by_slug(pool, "missing").await.expect("query").is_none());
    }
}
