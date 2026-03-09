-- Create do_not_send table for suppression list
CREATE TABLE do_not_send (
    email_address TEXT PRIMARY KEY,
    reason TEXT NOT NULL,         -- 'bounce', 'complaint', 'unsubscribe'
    details TEXT                  -- Optional additional info (bounce type, etc.)
);

CREATE INDEX idx_do_not_send_reason ON do_not_send(reason);
