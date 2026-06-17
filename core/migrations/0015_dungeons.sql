CREATE TABLE IF NOT EXISTS dungeons (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    vault_path TEXT NOT NULL UNIQUE,
    premise TEXT NOT NULL,
    topology TEXT NOT NULL,
    tone TEXT NOT NULL,
    twist TEXT NOT NULL,
    beats_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_dungeons_slug ON dungeons(slug);
CREATE INDEX IF NOT EXISTS idx_dungeons_name ON dungeons(name);
