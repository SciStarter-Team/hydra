-- Create pagebuilder_pages table for storing pages built from components
CREATE TABLE pagebuilder_pages (
    page_id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL,
    created TEXT NOT NULL,
    updated TEXT NOT NULL
);

-- Unique index for page names (case-insensitive)
CREATE UNIQUE INDEX pagebuilder_pages_by_name ON pagebuilder_pages (LOWER(name));

-- Index for ordering by updated time
CREATE INDEX pagebuilder_pages_by_updated ON pagebuilder_pages (updated);
