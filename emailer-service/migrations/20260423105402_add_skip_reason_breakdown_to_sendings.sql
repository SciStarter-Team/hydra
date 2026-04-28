-- Break out messages_skipped into per-reason counters so the admin can see
-- why a campaign lost recipients. messages_skipped continues to be the
-- grand total (sum of the per-reason columns below) for backward compatibility.

ALTER TABLE sendings ADD COLUMN skipped_unsubscribed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sendings ADD COLUMN skipped_bounced      INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sendings ADD COLUMN skipped_complained   INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sendings ADD COLUMN skipped_unverified   INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sendings ADD COLUMN skipped_opted_out    INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sendings ADD COLUMN skipped_invalid      INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sendings ADD COLUMN send_failed          INTEGER NOT NULL DEFAULT 0;
