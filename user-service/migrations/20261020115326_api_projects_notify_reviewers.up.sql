-- Whether the project reviewers hear of each project an account adds through
-- the partner API with its key. On by default; off for an account that only
-- exercises the API (a partner's developer, say) and would flood them.
-- Mirrors orrery's User.api_projects_notify_reviewers, and comes across in the
-- user sync until hydra is the primary.
ALTER TABLE "users" ADD COLUMN "api_projects_notify_reviewers" INTEGER NOT NULL DEFAULT 1;
