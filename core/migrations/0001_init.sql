CREATE TABLE IF NOT EXISTS documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_type TEXT NOT NULL,
    slug TEXT NOT NULL,
    title TEXT,
    vault_path TEXT NOT NULL UNIQUE,
    tags TEXT,
    created_at TEXT,
    updated_at TEXT,
    indexed_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_documents_doc_type ON documents(doc_type);
CREATE INDEX IF NOT EXISTS idx_documents_slug ON documents(slug);

CREATE TABLE IF NOT EXISTS generations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT NOT NULL,
    entity_id TEXT,
    prompt TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
