-- When a linked iNaturalist or Zooniverse account was connected to this one.
--
-- The floor for importing that platform's activity, now that August 2026
-- ended the backfilling of what happened before the connection. Deliberately
-- left null for the accounts already connected when the policy took effect:
-- their connection time was never recorded, and null is what
-- common::linked_platform reads as "connected before the cutover" and floors
-- there.
alter table "users" add column "inat_linked_at" text;
alter table "users" add column "zooniverse_linked_at" text;
