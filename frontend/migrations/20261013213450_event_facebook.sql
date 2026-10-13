-- What an event's organizer said about promoting it on Facebook, asked on
-- the add-event form (crate::event::facebook::FacebookEvent).
--
-- facebook_event: NULL until answered; 1 when the event has a Facebook event
-- of its own, 0 when it doesn't. Each answer has its own follow-up, and the
-- checks keep a follow-up from outliving a change of answer:
--
-- facebook_event_url: the event's Facebook event, for SciStarter to share.
-- facebook_event_requested: whether the organizer asked SciStarter to create
-- a Facebook event for them; NULL until they say.

ALTER TABLE events ADD COLUMN facebook_event BOOLEAN;

ALTER TABLE events ADD COLUMN facebook_event_url TEXT
    CHECK (facebook_event_url IS NULL OR facebook_event = 1);

ALTER TABLE events ADD COLUMN facebook_event_requested BOOLEAN
    CHECK (facebook_event_requested IS NULL OR facebook_event = 0);
