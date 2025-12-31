-- Create pagebuilder_components table for storing reusable page components
CREATE TABLE pagebuilder_components (
    component_id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'general',
    created TEXT NOT NULL,
    updated TEXT NOT NULL
);

-- Unique index for component names (case-insensitive)
CREATE UNIQUE INDEX pagebuilder_components_by_name ON pagebuilder_components (LOWER(name));

-- Index for filtering by category
CREATE INDEX pagebuilder_components_by_category ON pagebuilder_components (category);

-- Index for ordering by updated time
CREATE INDEX pagebuilder_components_by_updated ON pagebuilder_components (updated);
