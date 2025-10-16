CREATE TABLE oneoff_emails (
    oneoff_email_id TEXT PRIMARY KEY,  -- ULID
    subject TEXT NOT NULL,
    body_delta TEXT NOT NULL,          -- Quill Delta JSON
    created_at TEXT NOT NULL,          -- TimeString
    archived INTEGER NOT NULL DEFAULT 0  -- 0 = active, 1 = archived
);

CREATE INDEX idx_oneoff_emails_archived ON oneoff_emails(archived);
CREATE INDEX idx_oneoff_emails_created_at ON oneoff_emails(created_at);
