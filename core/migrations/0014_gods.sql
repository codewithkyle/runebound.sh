CREATE TABLE IF NOT EXISTS gods (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    vault_path TEXT NOT NULL UNIQUE,
    epithet TEXT NOT NULL,
    rank TEXT NOT NULL,
    rank_custom TEXT,
    alignment TEXT NOT NULL,
    domains TEXT NOT NULL,
    symbol TEXT NOT NULL,
    appearance TEXT NOT NULL,
    dogma TEXT NOT NULL,
    realm TEXT NOT NULL,
    worshippers TEXT NOT NULL,
    clergy TEXT NOT NULL,
    allies TEXT NOT NULL,
    rivals TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_gods_slug ON gods(slug);
CREATE INDEX IF NOT EXISTS idx_gods_name ON gods(name);
