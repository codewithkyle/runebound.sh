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

pub async fn find_faction_by_name_or_slug(pool: &SqlitePool, input: &str) -> Result<Option<FactionRow>> {
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
            undone_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
    .execute(pool)
    .await
    .context("failed to insert soft delete row")?;

    Ok(result.last_insert_rowid())
}

pub async fn latest_pending_soft_delete(pool: &SqlitePool) -> Result<Option<SoftDeleteRow>> {
    let row = sqlx::query(
        "SELECT id, entity_type, entity_id, name, slug, original_vault_path, trash_vault_path, payload_json, created_at, undone_at
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
    })
}
