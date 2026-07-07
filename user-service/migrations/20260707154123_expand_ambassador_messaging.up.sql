-- Contact-an-ambassador messages: a staff-moderated queue. A logged-in visitor
-- sends a message to a directory ambassador; staff review it and, once accepted,
-- it is emailed to the ambassador. Mirrors orrery's `AmbassadorMessage`.
CREATE TABLE "ambassador_messages" (
    "id"         TEXT PRIMARY KEY NOT NULL,
    "when"       TEXT NOT NULL DEFAULT (current_timestamp),
    "sender"     TEXT NOT NULL,
    "recipient"  TEXT NOT NULL,
    "first_name" TEXT NOT NULL DEFAULT '',
    "last_name"  TEXT NOT NULL DEFAULT '',
    "subject"    TEXT NOT NULL DEFAULT '',
    "dates"      TEXT NOT NULL DEFAULT '',
    "body"       TEXT NOT NULL DEFAULT '',
    "accepted"   INTEGER NOT NULL DEFAULT 0,
    "sent"       TEXT
);

CREATE INDEX "idx_ambassador_messages_recipient" ON "ambassador_messages" ("recipient");
CREATE INDEX "idx_ambassador_messages_pending" ON "ambassador_messages" ("accepted", "sent");

-- The native ambassador state-machine state (orrery's AmbassadorState.state).
-- Tracked so a staff review can detect a genuine state transition and fire the
-- matching entry email exactly once, the way orrery's state machine does. Both
-- the native review path and the orrery→hydra sync keep this current; only the
-- native path sends email.
ALTER TABLE "ambassador_progress" ADD COLUMN "state" TEXT NOT NULL DEFAULT 'initial';
