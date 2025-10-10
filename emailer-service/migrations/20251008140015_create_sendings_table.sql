-- Create sendings table for tracking bulk email campaigns
CREATE TABLE sendings (
    sending_id TEXT PRIMARY KEY,  -- ULID
    created_at TEXT NOT NULL,     -- TimeString
    template_name TEXT NOT NULL,  -- For reference/debugging
    total_queued INTEGER NOT NULL DEFAULT 0,
    messages_sent INTEGER NOT NULL DEFAULT 0,
    messages_skipped INTEGER NOT NULL DEFAULT 0,
    messages_opened INTEGER NOT NULL DEFAULT 0,
    messages_bounced INTEGER NOT NULL DEFAULT 0,
    unsubscribes INTEGER NOT NULL DEFAULT 0,
    complaints INTEGER NOT NULL DEFAULT 0
);
