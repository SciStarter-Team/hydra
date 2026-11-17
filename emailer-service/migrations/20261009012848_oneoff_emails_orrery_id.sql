-- A one-off copied from orrery's message templates carries orrery's id for
-- the template, which is what the sync keys it on: orrery can rename a
-- template's slug, but not its id. One made here carries none.
ALTER TABLE oneoff_emails ADD COLUMN orrery_id INTEGER;
CREATE UNIQUE INDEX idx_oneoff_emails_orrery_id ON oneoff_emails(orrery_id)
    WHERE orrery_id IS NOT NULL;

-- Whether orrery sends the template without an Unsubscribe link. A one-off
-- is sent here as a mailing, which always offers one; the setting is kept so
-- that a stored email made from the copy carries it.
ALTER TABLE oneoff_emails ADD COLUMN no_unsubscribe BOOLEAN NOT NULL DEFAULT 0;

-- The slug is now only the template's name, and two rows may share one: an
-- archived copy of a template orrery deleted keeps its slug after orrery
-- gives the slug to another.
DROP INDEX idx_oneoff_emails_legacy_slug;
CREATE INDEX idx_oneoff_emails_legacy_slug ON oneoff_emails(legacy_slug)
    WHERE legacy_slug IS NOT NULL;
