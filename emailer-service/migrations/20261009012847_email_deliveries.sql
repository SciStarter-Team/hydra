-- One row for each measured email handed to SES, for counting open rates.
--
-- This table deliberately has no way back to a recipient, and the privacy
-- policy, which speaks of usage data in aggregate, depends on that. Besides
-- having no address or user id:
--
-- * sent_on is a date, and token is random and the only key (WITHOUT ROWID
--   keeps no insertion order), because an exact time or a running number could
--   be lined up with send_history, which does name the recipient.
-- * sending_id is empty unless the sending went to sets of people, because a
--   triggered sending has one recipient, whom send_history names.
CREATE TABLE email_deliveries (
    token TEXT PRIMARY KEY NOT NULL,
    label TEXT NOT NULL,                 -- Which email this is
    sending_id TEXT,
    sent_on TEXT NOT NULL,               -- YYYY-MM-DD
    opens INTEGER NOT NULL DEFAULT 0,    -- Every load of the logo
    first_opened_at TEXT,
    first_open_agent TEXT                -- Tells an image proxy or scanner from a mail client
) WITHOUT ROWID;

CREATE INDEX idx_email_deliveries_sent_on ON email_deliveries(sent_on);

-- What a sending's messages count under, or NULL when they aren't measured.
ALTER TABLE sendings ADD COLUMN open_label TEXT;

-- Whether messages_opened is kept for this sending.
ALTER TABLE sendings ADD COLUMN opens_by_sending BOOLEAN NOT NULL DEFAULT 0;

ALTER TABLE scheduled_sends ADD COLUMN open_label TEXT;
