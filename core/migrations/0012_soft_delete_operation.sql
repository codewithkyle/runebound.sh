ALTER TABLE soft_deletes ADD COLUMN operation TEXT NOT NULL DEFAULT 'delete';

CREATE INDEX IF NOT EXISTS idx_soft_deletes_operation ON soft_deletes(operation);
