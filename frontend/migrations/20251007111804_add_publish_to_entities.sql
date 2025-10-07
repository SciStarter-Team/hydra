-- Add publish column to entities table
-- This centralizes publication control at the entity level

ALTER TABLE entities ADD COLUMN publish BOOLEAN NOT NULL DEFAULT 1;

-- Create index for efficient filtering by publish status
CREATE INDEX idx_entities_publish ON entities (publish);

-- Create composite index for kind + publish queries (common pattern)
CREATE INDEX idx_entities_kind_publish ON entities (kind, publish);
