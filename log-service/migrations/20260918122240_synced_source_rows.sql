-- Which orrery rows the importers have already turned into logged events.
--
-- A delivery is not an event. Two of orrery's sync feeds, joins and bookmarks,
-- page on `activity` -- a column orrery mutates whenever the row is touched --
-- so a row crosses the watermark again every time it changes and the importer
-- is handed the same event repeatedly. Logging per delivery turned one join
-- into as many as 715 identically-timestamped rows.
--
-- This table is the importer's own record of what it has consumed, which is
-- why the source id lives here rather than on the logged row: the log stays a
-- log of events, carrying no import plumbing, and nothing about it has to
-- change when hydra becomes the source of new joins. The table holds only
-- orrery ids and retires with the importers at cutover.
--
-- Feeds that page on the immutable `when` (participation, ratings, reviews,
-- golinks, acts of science) cannot re-deliver and do not appear here.
CREATE TABLE synced_source_rows (
    -- Which feed the id belongs to; ids are only unique within one.
    "feed"      TEXT NOT NULL,
    -- The primary key of the row in orrery's own table.
    "source_id" INTEGER NOT NULL,
    PRIMARY KEY ("feed", "source_id")
) WITHOUT ROWID;
