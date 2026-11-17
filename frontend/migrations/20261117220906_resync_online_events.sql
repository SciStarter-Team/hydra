-- Rewind the event sync again so orrery is asked for every event again.
--
-- Orrery's events feed read the raw `location_type`, in which 888 older
-- events still hold "ON" (online only), 791 of them with `entirely_online`
-- unset. It sent them with an empty `location_type` and the stale flag, so
-- hydra holds them as neither online nor anywhere, and the event page can't
-- show them as virtual. Orrery's fix sends "ON" as anywhere and entirely
-- online, but the incremental bookmark delivers that only for events that
-- change from then on. The replay made by 20261116165545 ran before the fix.
--
-- This release must therefore follow orrery's, or the replay fetches the
-- same stale values again.
--
-- Deleting the rows rather than assigning them restores the defaults the
-- sync itself falls back to. The misc table is created by
-- common::shared::init, which runs before migrations.
DELETE FROM misc WHERE key IN (
    'event-sync-bookmark-update',
    'event-sync-bookmark-id'
);
