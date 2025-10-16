CREATE TABLE stored_emails (
    stored_email_id TEXT PRIMARY KEY,  -- ULID
    slug TEXT NOT NULL UNIQUE,
    subject TEXT NOT NULL,
    body_delta TEXT NOT NULL,          -- Quill Delta JSON
    created_at TEXT NOT NULL,          -- TimeString
    updated_at TEXT NOT NULL           -- TimeString
);

CREATE INDEX idx_stored_emails_slug ON stored_emails(slug);
CREATE INDEX idx_stored_emails_created_at ON stored_emails(created_at);
