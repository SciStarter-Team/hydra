-- Every event the emailer has been told of, per user, with the earliest time
-- it was said to have happened.
--
-- A delayed binding is not scheduled for a user who has already been through
-- one of its cancellation events. Events from hydra's own code arrive in the
-- order they happen, but the sync replays orrery's history table by table, so
-- a cancelling event can arrive before the one it cancels; this is what lets
-- it count anyway.
CREATE TABLE trigger_events_seen (
    user_id TEXT NOT NULL,
    trigger_name TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    PRIMARY KEY (user_id, trigger_name)
) WITHOUT ROWID;
