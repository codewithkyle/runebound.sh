CREATE TABLE IF NOT EXISTS npcs (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    race TEXT NOT NULL,
    sex TEXT NOT NULL,
    location TEXT NOT NULL,
    vault_path TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_npcs_slug ON npcs(slug);
CREATE INDEX IF NOT EXISTS idx_npcs_name ON npcs(name);
CREATE INDEX IF NOT EXISTS idx_npcs_location ON npcs(location);
