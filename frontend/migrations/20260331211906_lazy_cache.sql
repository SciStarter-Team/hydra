CREATE TABLE IF NOT EXISTS lazy_cache (
    cache_key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
