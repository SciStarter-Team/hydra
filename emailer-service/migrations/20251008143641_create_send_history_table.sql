-- Create send_history table for audit trail of all sends
CREATE TABLE send_history (
    history_id TEXT PRIMARY KEY,         -- ULID
    queue_id TEXT NOT NULL,              -- References send_queue(queue_id)
    sending_id TEXT NOT NULL,            -- References sendings(sending_id)
    email_address TEXT NOT NULL,         -- Final resolved email
    sent_at TEXT,                        -- TimeString when sent (NULL if skipped/failed)
    status TEXT NOT NULL,                -- 'sent', 'skipped', 'failed'
    skip_reason TEXT,                    -- 'suppressed', 'invalid_data', etc.
    ses_message_id TEXT,                 -- AWS SES message ID
    FOREIGN KEY (sending_id) REFERENCES sendings(sending_id)
);

CREATE INDEX idx_send_history_sending_id ON send_history(sending_id);
CREATE INDEX idx_send_history_email ON send_history(email_address);
