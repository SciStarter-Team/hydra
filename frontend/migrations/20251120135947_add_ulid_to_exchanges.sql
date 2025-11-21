-- Restructure exchanges table to use ULID as primary key
-- exchange_id: ULID (primary key, the real identifier)
-- path_id: INTEGER (unique, for backward-compatible URLs)
--
-- NOTE: Existing exchange data should be deleted manually before running this migration

-- Drop related tables first (due to foreign keys)
DROP TABLE IF EXISTS exchange_hidden_projects;
DROP TABLE IF EXISTS exchange_predefined_searches;
DROP TABLE IF EXISTS exchanges;

CREATE TABLE exchanges (
    exchange_id TEXT PRIMARY KEY,  -- ULID
    path_id INTEGER NOT NULL UNIQUE,  -- For URL paths
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    logo_url TEXT,
    settings TEXT NOT NULL  -- JSON blob
) STRICT;

-- Create index on path_id for lookups
CREATE INDEX idx_exchanges_path_id ON exchanges(path_id);

-- Recreate related tables with updated foreign keys
CREATE TABLE exchange_predefined_searches (
    search_id TEXT PRIMARY KEY,  -- ULID
    exchange_id TEXT NOT NULL REFERENCES exchanges(exchange_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    search_params TEXT NOT NULL,  -- JSON serialized SearchParams
    display_order INTEGER NOT NULL
) STRICT;

CREATE TABLE exchange_hidden_projects (
    exchange_id TEXT NOT NULL REFERENCES exchanges(exchange_id) ON DELETE CASCADE,
    project_id TEXT NOT NULL,  -- ULID
    hidden_by TEXT NOT NULL,   -- user_id ULID
    hidden_at TEXT NOT NULL,   -- TimeString
    PRIMARY KEY (exchange_id, project_id)
) STRICT;
