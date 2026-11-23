-- Whether attending an event is free, paid or a suggested donation, and
-- whether people must register for it, as orrery's event form asks them
-- (core 0297). Each is empty when the event was never asked, as every
-- event saved before these questions was: such an event keeps the free-text
-- `cost` its organizer gave, and its `ticket_url` alone. Once admission is
-- answered, `cost` is the price or the suggested donation.
ALTER TABLE events ADD COLUMN admission TEXT NOT NULL DEFAULT ''
    CHECK (admission IN ('', 'free', 'paid', 'donation'));
ALTER TABLE events ADD COLUMN registration TEXT NOT NULL DEFAULT ''
    CHECK (registration IN ('', 'required', 'none', 'optional'));

-- Rewind the event sync so orrery is asked for every event again, and the
-- events already answered there arrive with their answers. Deleting the
-- rows rather than assigning them restores the defaults the sync itself
-- falls back to.
DELETE FROM misc WHERE key IN (
    'event-sync-bookmark-update',
    'event-sync-bookmark-id'
);
