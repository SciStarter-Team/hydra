-- When the user sync brought the account's current Panoptes id here from
-- orrery. Null when the id was found here, by asking Panoptes, or arrived
-- before this was recorded.
--
-- orrery identifies an account the moment it connects, but the sync carries
-- the id over only on its next pass, and until then log-service's bridge
-- cannot match the account's classifications. orrery credits them all the
-- same, so the bridge leaves what came before this to orrery's copy.
alter table "users" add column "zooniverse_id_received_at" text;
