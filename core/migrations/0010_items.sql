CREATE TABLE IF NOT EXISTS items (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    vault_path TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL,
    rarity TEXT NOT NULL,
    attunement TEXT NOT NULL,
    materials TEXT NOT NULL,
    appearance TEXT NOT NULL,
    abilities TEXT NOT NULL,
    drawbacks TEXT NOT NULL,
    history TEXT NOT NULL,
    value_gp TEXT NOT NULL,
    current_owner TEXT NOT NULL,
    location TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_items_slug ON items(slug);
CREATE INDEX IF NOT EXISTS idx_items_name ON items(name);
