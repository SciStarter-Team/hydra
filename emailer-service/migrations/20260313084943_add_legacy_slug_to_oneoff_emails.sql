ALTER TABLE oneoff_emails ADD COLUMN legacy_slug TEXT;
CREATE UNIQUE INDEX idx_oneoff_emails_legacy_slug ON oneoff_emails(legacy_slug) WHERE legacy_slug IS NOT NULL;
