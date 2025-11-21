-- Change search_id from INTEGER AUTOINCREMENT to TEXT (ULID) for consistency

-- Drop and recreate the table since we can't alter primary key
DROP TABLE IF EXISTS exchange_predefined_searches;

CREATE TABLE IF NOT EXISTS exchange_predefined_searches (
    search_id TEXT PRIMARY KEY,  -- ULID
    exchange_id INTEGER NOT NULL REFERENCES exchanges(exchange_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    search_params TEXT NOT NULL,  -- JSON serialized SearchParams
    display_order INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS exchange_searches_by_exchange
    ON exchange_predefined_searches(exchange_id);
