-- Bookkeeping for the Constant Contact sync, which keeps their copy of a
-- person's details current for targeting and mail merge.
--
-- `constant_contact_dirty` is the sweep's work list: set when an account
-- changes, cleared once their record has been brought up to date. It defaults
-- to 0 rather than 1, unlike the equivalent column in orrery, because orrery
-- has already done the backfill and carries its digest across in the user
-- sync -- so an account arrives here already current, and flagging the whole
-- table would spend a month of the daily request allowance re-sending what
-- Constant Contact already holds.
--
-- `constant_contact_digest` fingerprints what was last sent, so a change that
-- alters nothing they hold costs no request at all.
ALTER TABLE "users" ADD COLUMN "constant_contact_dirty" integer NOT NULL DEFAULT 0;
ALTER TABLE "users" ADD COLUMN "constant_contact_digest" text NOT NULL DEFAULT '';

-- Named from the account's location rather than typed by the person, so that
-- staff can segment on a state instead of on the leading digits of a postal
-- code.
ALTER TABLE "users" ADD COLUMN "state" text NOT NULL DEFAULT '';

-- Partial, because the sweep only ever asks for the flagged rows.
CREATE INDEX "users_pending_constant_contact"
    ON "users" ("constant_contact_dirty")
    WHERE "constant_contact_dirty" = 1;
