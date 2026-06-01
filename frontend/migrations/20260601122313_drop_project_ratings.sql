-- The frontend-local project_ratings system has been removed. Likes are now
-- owned by the log-service (project_ratings_cache), which unifies orrery-synced
-- rating history with new hydra likes and is queried over NATS.
DROP INDEX IF EXISTS project_ratings_by_user;
DROP INDEX IF EXISTS project_ratings_by_project;
DROP TABLE IF EXISTS project_ratings;
