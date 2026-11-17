-- A project's managers star participants to find them again (orrery's
-- Participant Manager). Orrery keeps the star in the `active` column of its
-- joins and bookmarks; a save it no longer holds is deleted, not cleared.
-- Here a bookmark's row standing is what makes it a save, and the star is a
-- column of its own on bookmarks and joins alike.
ALTER TABLE "bookmarks" RENAME COLUMN "active" TO "starred";

ALTER TABLE "project_joins" ADD COLUMN "starred" BOOLEAN NOT NULL DEFAULT 1;

-- When the participant last agreed to the project's consent statement.
ALTER TABLE "project_joins" ADD COLUMN "consented" TEXT;

-- The join sync took orrery's star for whether a join stood, and dropped
-- the joins of unstarred participants. Syncing joins and bookmarks again
-- from the start restores those joins and carries in the stars and consents.
DELETE FROM "misc" WHERE "key" IN (
    'join-sync-bookmark', 'join-sync-last-id',
    'bookmark-sync-bookmark', 'bookmark-sync-last-id'
);
