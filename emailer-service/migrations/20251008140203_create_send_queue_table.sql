-- Create send_queue table for queuing individual email sends
CREATE TABLE send_queue (
    queue_id TEXT PRIMARY KEY,           -- ULID
    sending_id TEXT NOT NULL,            -- References sendings(sending_id)
    target_type TEXT NOT NULL,           -- 'email' or 'user_id'
    target_value TEXT NOT NULL,          -- Email address or user ULID
    data_sets TEXT NOT NULL,             -- JSON array of data set names
    template_html TEXT NOT NULL,         -- HTML template with merge fields
    queued_at TEXT NOT NULL,             -- TimeString
    status TEXT NOT NULL DEFAULT 'pending',  -- 'pending' or 'processing'
    FOREIGN KEY (sending_id) REFERENCES sendings(sending_id)
);

CREATE INDEX idx_send_queue_status ON send_queue(status);
CREATE INDEX idx_send_queue_sending_id ON send_queue(sending_id);
