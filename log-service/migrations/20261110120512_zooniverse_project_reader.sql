-- Which of SciStarter's Zooniverse accounts each project's team made a
-- collaborator, and so the one its classifications are read as: 'legacy',
-- the account the bridge has used from the start, or 'scistarter', the one
-- new projects are connected to. Every project here was connected while the
-- first was the only one. The project sync from orrery keeps this current.
ALTER TABLE zooniverse_projects ADD COLUMN "reader" TEXT NOT NULL DEFAULT 'legacy';
