-- What the participation importer has logged for each of orrery's events.
--
-- An event in orrery is not written once: later contributions fold into its
-- `repetitions`, repairs rewrite it, deleting an account deletes it. orrery
-- numbers every one of those writes, and the importer follows them in order
-- (see `participation_feed`), comparing each with what it logged last time:
-- more repetitions are logged as more rows, and anything else takes back
-- what was logged and logs the event as it now stands.
CREATE TABLE participation_sources (
    -- The id of the event in orrery's `affiliate_event`.
    "source_id" INTEGER PRIMARY KEY NOT NULL,
    -- The row logged for each repetition, as JSON exactly as it was written,
    -- so that more repetitions are logged identically and a retraction can
    -- find the rows it takes back. NULL when nothing is logged.
    "row"       TEXT,
    -- How many copies of "row" are in the log.
    "logged"    INTEGER NOT NULL,
    -- Whether a bridge here has been the importer of record for the event.
    -- Once it has, it stays so: the bridge's rows outlive the link that
    -- produced them, so importing the event again would count it twice.
    "covered"   INTEGER NOT NULL
);

-- Rows that orrery has taken back, waiting to be removed from the log.
--
-- The log is written append-only, so taking rows back is done by the
-- compactor, which rewrites the Parquet files holding them. A row still in
-- the WAL is removed once it has been compacted.
CREATE TABLE participation_retractions (
    "retraction_id" INTEGER PRIMARY KEY NOT NULL,
    -- The row to remove, as JSON exactly as it was written.
    "row"           TEXT NOT NULL,
    -- How many copies of it are still to be removed.
    "remaining"     INTEGER NOT NULL,
    "queued_at"     TEXT NOT NULL,
    "reason"        TEXT NOT NULL,
    -- When the compactor last looked through the log for it, in Unix
    -- milliseconds: files older than that need not be read for it again.
    -- NULL until it first looks.
    "scanned_at"    INTEGER
);

CREATE INDEX participation_retractions_pending
    ON participation_retractions ("retraction_id") WHERE "remaining" > 0;

-- Renames the compactor has committed to and not yet confirmed.
--
-- Removing rows from a file and recording that they are removed are two
-- writes to two places. The replacement is written first, the rename is
-- recorded here in the same transaction as the counts it settles, and the
-- rename is made afterwards; a rename still recorded here when the compactor
-- next runs is made then, before anything else.
CREATE TABLE log_file_moves (
    "move_id" INTEGER PRIMARY KEY NOT NULL,
    "from"    TEXT NOT NULL,
    "to"      TEXT NOT NULL
);

-- The earliest classification the Zooniverse bridge credited to the project
-- before the August 2026 import policy, or NULL if it credited none. Until
-- the policy, the bridge credited every connected account, so for an account
-- connected before the policy, orrery's Zooniverse participation from then on
-- is already in the log. Filled in by `participation-repair`.
ALTER TABLE zooniverse_projects ADD COLUMN "credited_since" TEXT;

-- Participation no longer uses the claim ledger; `participation_sources`
-- replaces it.
DELETE FROM synced_source_rows WHERE "feed" = 'participation';
