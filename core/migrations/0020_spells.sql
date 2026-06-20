-- Spell search index. The full card payload lives in the canonical TOML spell
-- store (config/runebound.sh/spells/<slug>.toml); this table holds only the
-- columns the typeahead + lookup search on, mirroring how the entity tables are a
-- rebuildable projection of the entity store. `id` = slug.
CREATE TABLE IF NOT EXISTS spells (
    id            TEXT PRIMARY KEY,
    slug          TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    level         INTEGER NOT NULL,
    school        TEXT NOT NULL,
    source        TEXT NOT NULL,
    ritual        INTEGER NOT NULL DEFAULT 0,
    concentration INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_spells_name ON spells(name);
CREATE INDEX IF NOT EXISTS idx_spells_level ON spells(level);
CREATE INDEX IF NOT EXISTS idx_spells_school ON spells(school);
