-- Remove UNIQUE constraint from trigger_name to allow multiple triggers for the same event
-- SQLite requires recreating the table to remove constraints

-- Create new table without UNIQUE constraint on trigger_name
CREATE TABLE email_triggers_new (
    trigger_id TEXT PRIMARY KEY NOT NULL,
    trigger_name TEXT NOT NULL,
    stored_email_id TEXT NOT NULL,
    action_type TEXT NOT NULL CHECK(action_type IN ('immediate', 'delayed')),
    delay_seconds INTEGER,
    cancellation_trigger_name TEXT,
    active BOOLEAN NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Copy data from old table
INSERT INTO email_triggers_new
SELECT trigger_id, trigger_name, stored_email_id, action_type, delay_seconds,
       cancellation_trigger_name, active, created_at, updated_at
FROM email_triggers;

-- Drop old table
DROP TABLE email_triggers;

-- Rename new table
ALTER TABLE email_triggers_new RENAME TO email_triggers;

-- Recreate indexes (trigger_name index now without UNIQUE constraint)
CREATE INDEX idx_email_triggers_trigger_name ON email_triggers(trigger_name) WHERE active = 1;
CREATE INDEX idx_email_triggers_cancellation ON email_triggers(cancellation_trigger_name) WHERE cancellation_trigger_name IS NOT NULL AND active = 1;
