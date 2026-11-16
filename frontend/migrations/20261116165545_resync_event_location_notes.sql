-- Rewind the event sync so orrery is asked for every event again.
--
-- Orrery sent every event's `location_description` empty until its release
-- with core 0294: it read the note as ShortText, and an event's is Text.
-- That release sends the notes, but the incremental bookmark only delivers
-- them for events that change from then on, so the notes already written
-- (127 events at the time) arrive only if the whole history is replayed.
-- The same pass brings the new `contact_url` of every event that has one.
--
-- This release must therefore follow orrery's, or the replay fetches the
-- empty notes again.
--
-- Deleting the rows rather than assigning them restores the defaults the
-- sync itself falls back to. The misc table is created by
-- common::shared::init, which runs before migrations.
DELETE FROM misc WHERE key IN (
    'event-sync-bookmark-update',
    'event-sync-bookmark-id'
);
