CREATE TABLE geocode_cache (
    id TEXT PRIMARY KEY NOT NULL,
    query TEXT NOT NULL,
    formatted TEXT NOT NULL,
    latitude REAL NOT NULL,
    longitude REAL NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(query, formatted)
);

CREATE INDEX idx_geocode_cache_query ON geocode_cache(query);
