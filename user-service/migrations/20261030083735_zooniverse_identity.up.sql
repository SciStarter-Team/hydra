-- The Panoptes account a Zooniverse username names, which is what a
-- classification is attributed to, and when Panoptes was last asked about
-- the username whether or not it knew it. Both describe the account the
-- username names, so they are cleared whenever it changes.
--
-- While orrery is primary it identifies the accounts and the user sync
-- carries the id here; once hydra is, log-service asks Panoptes itself.
alter table "users" add column "zooniverse_id" integer;
alter table "users" add column "zooniverse_checked_at" text;
