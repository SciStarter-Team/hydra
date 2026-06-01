-- The frontend-local project_reviews system has been removed. Reviews are now
-- owned by the log-service (project_reviews_cache), which unifies orrery-synced
-- review history with new hydra reviews and is queried over NATS.
DROP INDEX IF EXISTS project_reviews_by_user;
DROP INDEX IF EXISTS project_reviews_by_project;
DROP TABLE IF EXISTS project_reviews;
