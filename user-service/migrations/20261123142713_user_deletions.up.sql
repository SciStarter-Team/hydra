-- The accounts deleted, in the order they were deleted, for every service
-- to forget what it keeps about each (common::user_deletions). An account
-- deleted because orrery no longer has it carries orrery's id for it.
CREATE TABLE "deleted_users" (
    "seq" INTEGER PRIMARY KEY AUTOINCREMENT,
    "user_id" TEXT NOT NULL UNIQUE,
    "legacy_v3_id" INTEGER,
    "deleted_at" TEXT NOT NULL
);

-- Orrery deleted 14,573 accounts that never became real (Canvas LTI and
-- Clever) before it reported deletions, so the first sweep finds some 4.9%
-- of the accounts here gone, more than a sweep may delete unbidden. This
-- lets that one sweep delete them.
CREATE TABLE IF NOT EXISTS "misc" (
    "key" text primary key not null,
    "value" any not null
) strict;
INSERT OR REPLACE INTO "misc" ("key", "value") VALUES ('user-deletion-allowance', 15000);
