CREATE TABLE items_new (
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
    value TEXT NOT NULL,
    location TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO items_new (id, slug, name, vault_path, category, rarity, attunement, materials, appearance, abilities, drawbacks, history, value, location, created_at, updated_at)
SELECT id, slug, name, vault_path, category, rarity, attunement, materials, appearance, abilities, drawbacks, history, value_gp, location, created_at, updated_at
FROM items;

DROP TABLE items;

ALTER TABLE items_new RENAME TO items;

CREATE INDEX IF NOT EXISTS idx_items_slug ON items(slug);
CREATE INDEX IF NOT EXISTS idx_items_name ON items(name);