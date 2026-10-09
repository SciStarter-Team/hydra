-- Messages withheld because this build is not the primary server (see
-- common::primary): everything about the send happened except handing it to
-- SES. Counted apart from the other skips, and within messages_skipped like
-- them.
ALTER TABLE sendings ADD COLUMN skipped_not_primary INTEGER NOT NULL DEFAULT 0;
