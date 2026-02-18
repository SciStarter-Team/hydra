CREATE TABLE IF NOT EXISTS acts_of_science (
    act_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    email TEXT,
    country TEXT,
    newsletter INTEGER NOT NULL DEFAULT 0,
    name TEXT,
    contributions INTEGER NOT NULL DEFAULT 1,
    act TEXT,
    organization TEXT,
    num_others INTEGER NOT NULL DEFAULT 0,
    others TEXT,
    project TEXT,
    notes TEXT,
    source INTEGER NOT NULL DEFAULT 0,
    entity_slug TEXT,
    accepted INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX idx_acts_created_at ON acts_of_science(created_at);
CREATE INDEX idx_acts_source ON acts_of_science(source);
CREATE INDEX idx_acts_accepted ON acts_of_science(accepted);
CREATE INDEX idx_acts_source_accepted_created ON acts_of_science(source, accepted, created_at);
