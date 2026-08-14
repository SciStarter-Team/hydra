-- The name a participant gives when a list asks who they are, so that an
-- educator reading the list's stats sees a student rather than a handle.
--
-- Deliberately not part of entity_list_viewers: that table records who the
-- owner shared the list with, and a participant naming themselves is not a
-- grant of access. Sharing the table would put every self-identifying
-- participant into the owner's share list.
CREATE TABLE IF NOT EXISTS entity_list_identities (
    list_id TEXT NOT NULL REFERENCES entity_lists(list_id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    first_name TEXT NOT NULL DEFAULT '',
    last_name TEXT NOT NULL DEFAULT '',
    given_at TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (list_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_entity_list_identities_user_id
    ON entity_list_identities(user_id);
