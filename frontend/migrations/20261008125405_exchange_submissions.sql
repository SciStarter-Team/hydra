-- Events, like projects, can be submitted through an exchange and then wait
-- for it to approve or reject them. Events already here are not waiting:
-- the sync below files any that are.
ALTER TABLE events ADD COLUMN exchange TEXT;
ALTER TABLE events ADD COLUMN reviewed INTEGER NOT NULL DEFAULT 1;

-- Rewind the project and event syncs so orrery is asked for everything again.
--
-- Until this release the project sync filed every project as reviewed and
-- the event sync carried no exchange at all, so whatever is waiting in an
-- exchange's approval queue in orrery arrived here looking settled. Those
-- rows only come back if the cursors are moved behind them; orrery has not
-- touched most of them since.
--
-- Replaying the history is safe: projects and events are keyed by orrery's
-- ULIDs and written in place. Deleting the rows rather than assigning them
-- restores the defaults the syncs fall back to. The misc table is created by
-- common::shared::init, which runs before migrations.
DELETE FROM misc WHERE key IN (
    'project-sync-bookmark-update',
    'project-sync-bookmark-id',
    'event-sync-bookmark-update',
    'event-sync-bookmark-id'
);
