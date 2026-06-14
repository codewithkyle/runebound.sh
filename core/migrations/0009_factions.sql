CREATE TABLE IF NOT EXISTS factions (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    vault_path TEXT NOT NULL UNIQUE,
    kind_type TEXT NOT NULL,
    kind_custom TEXT,
    public_description TEXT NOT NULL,
    true_agenda TEXT NOT NULL,
    methods TEXT NOT NULL,
    leadership TEXT NOT NULL,
    headquarters TEXT NOT NULL,
    sphere_of_influence TEXT NOT NULL,
    resources_assets TEXT NOT NULL,
    allies TEXT NOT NULL,
    rivals_enemies TEXT NOT NULL,
    reputation TEXT NOT NULL,
    current_tension TEXT NOT NULL,
    goals_short_term TEXT NOT NULL,
    goals_long_term TEXT NOT NULL,
    symbol_description TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_factions_slug ON factions(slug);
CREATE INDEX IF NOT EXISTS idx_factions_name ON factions(name);
