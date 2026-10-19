-- The iNaturalist importer reads its project mapping from the frontend, where
-- `projects.inat_project_id` is kept. Nothing ever filled this copy.
DROP TABLE IF EXISTS inaturalist_projects;
