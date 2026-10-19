-- Walk every linked iNaturalist account again from its import floor.
--
-- The bridge credited every observation to a project named "inaturalist",
-- which is no project's id, so none of it counted anywhere; the log service's
-- participation-repair removes those rows. The bridge now credits each
-- observation to the projects it was recorded in, and its ledger keeps it
-- from crediting any observation twice, so walking again from the floor
-- credits the same observations to the right projects.
UPDATE "users" SET "inat_last_update" = NULL;
