-- Email triggers: automated email sending based on events
-- Uses existing scheduled_sends infrastructure for delayed sends
CREATE TABLE email_triggers (
    trigger_id TEXT PRIMARY KEY NOT NULL,
    trigger_name TEXT NOT NULL UNIQUE, -- Event name like "user.signup" or "project.submitted"
    stored_email_id TEXT NOT NULL, -- Reference to stored_emails table
    action_type TEXT NOT NULL CHECK(action_type IN ('immediate', 'delayed')),
    delay_seconds INTEGER, -- NULL for immediate, required for delayed
    cancellation_trigger_name TEXT, -- Optional: if this event occurs, cancel pending sends (uses hook in scheduled_sends)
    active BOOLEAN NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Index for looking up triggers by event name
CREATE INDEX idx_email_triggers_trigger_name ON email_triggers(trigger_name) WHERE active = 1;

-- Index for looking up cancellation triggers
CREATE INDEX idx_email_triggers_cancellation ON email_triggers(cancellation_trigger_name) WHERE cancellation_trigger_name IS NOT NULL AND active = 1;
