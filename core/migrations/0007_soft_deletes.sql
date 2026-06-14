CREATE TABLE IF NOT EXISTS soft_deletes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    original_vault_path TEXT NOT NULL,
    trash_vault_path TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    undone_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_soft_deletes_created_at ON soft_deletes(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_soft_deletes_undone_at ON soft_deletes(undone_at);
