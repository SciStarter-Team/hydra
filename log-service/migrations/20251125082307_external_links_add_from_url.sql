-- Add slug field to external_links and change unique constraint
-- Since SQLite doesn't support modifying constraints, we need to recreate the table

-- Create new table with slug field and composite unique constraint
CREATE TABLE external_links_new (
    link_id TEXT PRIMARY KEY NOT NULL,  -- ULID
    slug TEXT NOT NULL,                  -- The entity slug where this link was found
    url TEXT NOT NULL,                   -- The external URL being linked to
    created_at TEXT NOT NULL,            -- When this link was first registered
    UNIQUE(slug, url)                    -- Unique constraint on the combination
) STRICT;

-- Copy existing data (use empty string for slug as placeholder)
INSERT INTO external_links_new (link_id, slug, url, created_at)
SELECT link_id, '', url, created_at FROM external_links;

-- Drop old table
DROP TABLE external_links;

-- Rename new table
ALTER TABLE external_links_new RENAME TO external_links;

-- Create index for fast lookups by slug and url combination
CREATE INDEX external_links_by_slug_and_url ON external_links(slug, url);
