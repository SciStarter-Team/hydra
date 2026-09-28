-- Replay the syncs whose rows the user sync had been deleting.
--
-- The user sync wrote accounts with INSERT OR REPLACE. REPLACE deletes the
-- old row before inserting, and with foreign keys on, the delete cascaded to
-- every table declared `REFERENCES users ON DELETE CASCADE`. orrery touches
-- `core_user.updated` constantly, so each pass stripped the active accounts of
-- their ambassador applications, project joins and bookmarks, and those syncs
-- had long since bookmarked past the rows, so nothing put them back.
--
-- The user sync is now an upsert. Forgetting these bookmarks makes each of the
-- three mirrors read its orrery feed again from the beginning. All three are
-- idempotent upserts and deletes against a relationship table, so a replay
-- converges on orrery's current state.
--
-- Emails and tags are not listed: the user sync re-inserts them in the same
-- pass that deleted them. Rows hydra wrote itself (passkeys, places, link
-- tokens, native drafts) have no copy anywhere to replay from.
DELETE FROM "misc" WHERE "key" IN (
    'application-sync-bookmark',
    'application-sync-last-id',
    'join-sync-bookmark',
    'join-sync-last-id',
    'bookmark-sync-bookmark',
    'bookmark-sync-last-id'
);
