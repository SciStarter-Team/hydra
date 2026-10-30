-- Whether the account has contributed to the project, as distinct from only
-- having joined it. The first-contribution mail thanks a volunteer for their
-- first data, and a report that they joined is none, so the mail waits for
-- the row to turn from joined to contributed.
--
-- Rows already here count as contributed. Until now a report of joining sent
-- the mail like any other, and the mail goes to an account only once, so an
-- account whose first row was a joining has already been sent it: nothing is
-- owed to any account these rows describe.
ALTER TABLE "project_participations_cache"
    ADD COLUMN "contributed" INTEGER NOT NULL DEFAULT 1;
