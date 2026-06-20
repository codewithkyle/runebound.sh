-- Expanded Factions (v0.7.0): the WOAC rework. Old factions are not migrated
-- (design §10 / spec §0 — test data is wiped between releases), so this drops the
-- 0009 table and recreates it with the new column set (spec §1.3): the WOAC engine
-- (want/obstacle/action/consequence), the derived `category`, `leader`, and the
-- nullable houses-only `liege`/`loyalty_type`. The removed columns (kind_custom,
-- true_agenda, methods, headquarters, current_tension, goals_*) are gone.
DROP TABLE IF EXISTS factions;

CREATE TABLE factions (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    vault_path TEXT NOT NULL UNIQUE,
    kind_type TEXT NOT NULL,
    category TEXT NOT NULL,
    public_description TEXT NOT NULL,
    reputation TEXT NOT NULL,
    symbol_description TEXT NOT NULL,
    want TEXT NOT NULL,
    obstacle TEXT NOT NULL,
    action TEXT NOT NULL,
    consequence TEXT NOT NULL,
    leader TEXT NOT NULL,
    sphere_of_influence TEXT NOT NULL,
    resources_assets TEXT NOT NULL,
    allies TEXT NOT NULL,
    rivals_enemies TEXT NOT NULL,
    liege TEXT,
    loyalty_type TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_factions_slug ON factions(slug);
CREATE INDEX idx_factions_name ON factions(name);
CREATE INDEX idx_factions_category ON factions(category);
