ALTER TABLE locations ADD COLUMN kind_type TEXT NOT NULL DEFAULT 'other';
ALTER TABLE locations ADD COLUMN kind_custom TEXT;
ALTER TABLE locations ADD COLUMN visual_description TEXT NOT NULL DEFAULT 'Unknown';
ALTER TABLE locations ADD COLUMN history_background TEXT NOT NULL DEFAULT 'Unknown';
ALTER TABLE locations ADD COLUMN exports TEXT NOT NULL DEFAULT '["Unknown"]';
ALTER TABLE locations ADD COLUMN tone TEXT NOT NULL DEFAULT 'Unknown';
ALTER TABLE locations ADD COLUMN authority TEXT NOT NULL DEFAULT 'Unknown';
ALTER TABLE locations ADD COLUMN danger_level TEXT NOT NULL DEFAULT 'Unknown';
ALTER TABLE locations ADD COLUMN current_tension TEXT NOT NULL DEFAULT 'Unknown';
