use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use sqlx::migrate::Migrator;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Row, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub struct Database {
    pub pool: SqlitePool,
    pub path: PathBuf,
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
    Ok(data_dir.join("dnd-assistant").join("app.db"))
}
