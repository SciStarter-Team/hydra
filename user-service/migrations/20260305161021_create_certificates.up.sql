CREATE TABLE IF NOT EXISTS certificates (
    certificate_id TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL,
    user_id TEXT NOT NULL,
    project TEXT NOT NULL,
    mode TEXT NOT NULL,
    UNIQUE(user_id, project, mode)
);

CREATE INDEX IF NOT EXISTS idx_certificates_user_id ON certificates(user_id);
CREATE INDEX IF NOT EXISTS idx_certificates_mode ON certificates(mode);
