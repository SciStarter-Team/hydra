-- Every time a staff member takes on another user's identity.
--
-- orrery's equivalent (`/staff/ax4gr27`) left no trace at all: a staff account
-- could become any account and nothing recorded that it happened, so anything
-- done afterwards was indistinguishable from the real user doing it. The
-- capability is worth having and the silence is not, so hydra writes a row
-- before handing over the session.
--
-- Rows are never deleted or updated. What the masquerading session went on to
-- do is recorded by ordinary activity logging, against the target user; this
-- table is what lets that be traced back to a person.
CREATE TABLE "masquerades" (
    "masquerade_id" TEXT PRIMARY KEY,  -- ULID
    "when" TEXT NOT NULL,              -- TimeString
    "staff_user_id" TEXT NOT NULL,
    "target_user_id" TEXT NOT NULL
) STRICT;

CREATE INDEX "masquerades_by_staff" ON "masquerades" ("staff_user_id", "when");
CREATE INDEX "masquerades_by_target" ON "masquerades" ("target_user_id", "when");
