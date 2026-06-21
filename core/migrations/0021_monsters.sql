-- Monster search index. The full card payload lives in the canonical TOML monster
-- store (config/runebound.sh/monsters/<slug>.toml); this table holds only the
-- columns the typeahead + lookup search on, mirroring how the spell table is a
-- rebuildable projection of the spell store. `id` = slug. (`type` is reserved in
-- some contexts, so the column is `creature_type`.)
CREATE TABLE IF NOT EXISTS monsters (
    id            TEXT PRIMARY KEY,
    slug          TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    cr            TEXT NOT NULL DEFAULT '',
    cr_sort       REAL NOT NULL DEFAULT 0,  -- numeric CR for ordering ("1/4" -> 0.25)
    creature_type TEXT NOT NULL DEFAULT '',
    size          TEXT NOT NULL DEFAULT '',
    source        TEXT NOT NULL,
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_monsters_name ON monsters(name);
CREATE INDEX IF NOT EXISTS idx_monsters_cr   ON monsters(cr_sort);
CREATE INDEX IF NOT EXISTS idx_monsters_type ON monsters(creature_type);
