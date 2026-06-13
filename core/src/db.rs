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
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NpcRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub race: String,
    pub sex: String,
    pub location: String,
    pub vault_path: String,
    pub created_at: String,
    pub updated_at: String,
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
        "SELECT id, slug, name, vault_path, created_at, updated_at FROM locations WHERE slug = ?1",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .context("failed to query location by slug")?;

    row.map(row_to_location).transpose()
}

pub async fn upsert_location(pool: &SqlitePool, location: &LocationRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO locations (id, slug, name, vault_path, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
            slug = excluded.slug,
            name = excluded.name,
            vault_path = excluded.vault_path,
            updated_at = excluded.updated_at",
    )
    .bind(&location.id)
    .bind(&location.slug)
    .bind(&location.name)
    .bind(&location.vault_path)
    .bind(&location.created_at)
    .bind(&location.updated_at)
    .execute(pool)
    .await
    .context("failed to upsert location")?;

    Ok(())
}

pub async fn find_npc_by_id(pool: &SqlitePool, id: &str) -> Result<Option<NpcRow>> {
    let row = sqlx::query(
        "SELECT id, slug, name, race, sex, location, vault_path, created_at, updated_at FROM npcs WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("failed to query npc by id")?;

    row.map(row_to_npc).transpose()
}

pub async fn upsert_npc(pool: &SqlitePool, npc: &NpcRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO npcs (id, slug, name, race, sex, location, vault_path, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET
            slug = excluded.slug,
            name = excluded.name,
            race = excluded.race,
            sex = excluded.sex,
            location = excluded.location,
            vault_path = excluded.vault_path,
            updated_at = excluded.updated_at",
    )
    .bind(&npc.id)
    .bind(&npc.slug)
    .bind(&npc.name)
    .bind(&npc.race)
    .bind(&npc.sex)
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
        .map(|row| {
            row.try_get("prompt")
                .context("generations.prompt missing")
        })
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
        created_at: row
            .try_get("created_at")
            .context("locations.created_at missing")?,
        updated_at: row
            .try_get("updated_at")
            .context("locations.updated_at missing")?,
    })
}

fn row_to_npc(row: sqlx::sqlite::SqliteRow) -> Result<NpcRow> {
    Ok(NpcRow {
        id: row.try_get("id").context("npcs.id missing")?,
        slug: row.try_get("slug").context("npcs.slug missing")?,
        name: row.try_get("name").context("npcs.name missing")?,
        race: row.try_get("race").context("npcs.race missing")?,
        sex: row.try_get("sex").context("npcs.sex missing")?,
        location: row
            .try_get("location")
            .context("npcs.location missing")?,
        vault_path: row
            .try_get("vault_path")
            .context("npcs.vault_path missing")?,
        created_at: row.try_get("created_at").context("npcs.created_at missing")?,
        updated_at: row.try_get("updated_at").context("npcs.updated_at missing")?,
    })
}
