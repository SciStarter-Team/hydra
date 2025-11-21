-- Add reviewed column to projects table
-- This column tracks whether a project has been reviewed by staff
-- 0 (false) = not yet reviewed, needs staff review
-- 1 (true) = reviewed (either approved and published, or reviewed and kept unpublished)
-- Used by both exchange approval workflow and general site admin review

ALTER TABLE projects ADD COLUMN reviewed BOOLEAN NOT NULL DEFAULT 0;

-- Set reviewed = 1 for all existing projects
-- (assume they are already reviewed since they're in the system)
UPDATE projects SET reviewed = 1;
