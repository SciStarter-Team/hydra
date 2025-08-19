-- Summary metadata table for tracking incremental summary updates
CREATE TABLE summary_metadata (
    table_name TEXT PRIMARY KEY,
    last_processed_date TEXT NOT NULL,
    last_updated_at INTEGER NOT NULL
);

-- Index for faster lookups
CREATE INDEX idx_summary_metadata_updated_at ON summary_metadata(last_updated_at);