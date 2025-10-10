-- Create scheduled_sends table for delayed email delivery
CREATE TABLE scheduled_sends (
    scheduled_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL,
    hook TEXT NOT NULL,
    scheduled_time TEXT NOT NULL,
    template_html TEXT NOT NULL,
    subject TEXT NOT NULL,
    data_sets TEXT NOT NULL,
    additional_data TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- Index for cancellation lookups by user_id and hook
CREATE INDEX idx_scheduled_sends_user_hook ON scheduled_sends(user_id, hook);

-- Index for processor queries to find pending sends that are due
CREATE INDEX idx_scheduled_sends_time ON scheduled_sends(scheduled_time);
