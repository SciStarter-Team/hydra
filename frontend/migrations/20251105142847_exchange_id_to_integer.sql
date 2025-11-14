-- Change exchange_id from TEXT (ULID) to INTEGER for backward compatibility with orrery

-- Drop dependent tables first
DROP TABLE IF EXISTS exchange_hidden_projects;
DROP TABLE IF EXISTS exchange_predefined_searches;
DROP TABLE IF EXISTS exchanges;

-- Recreate with INTEGER exchange_id
CREATE TABLE IF NOT EXISTS exchanges (
    exchange_id INTEGER PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    logo_url TEXT,
    settings TEXT NOT NULL,  -- JSON blob
    created TEXT NOT NULL,   -- TimeString
    updated TEXT NOT NULL    -- TimeString
) STRICT;

CREATE TABLE IF NOT EXISTS exchange_predefined_searches (
    search_id INTEGER PRIMARY KEY AUTOINCREMENT,
    exchange_id INTEGER NOT NULL REFERENCES exchanges(exchange_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    search_params TEXT NOT NULL,  -- JSON serialized SearchParams
    display_order INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS exchange_searches_by_exchange
    ON exchange_predefined_searches(exchange_id);

CREATE TABLE IF NOT EXISTS exchange_hidden_projects (
    exchange_id INTEGER NOT NULL REFERENCES exchanges(exchange_id) ON DELETE CASCADE,
    project_id TEXT NOT NULL,  -- ULID
    hidden_by TEXT NOT NULL,   -- user_id ULID
    hidden_at TEXT NOT NULL,   -- TimeString
    PRIMARY KEY (exchange_id, project_id)
) STRICT;

CREATE INDEX IF NOT EXISTS exchange_hidden_by_exchange
    ON exchange_hidden_projects(exchange_id);
