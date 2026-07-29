-- Why a reviewer declined to publish a project, and which reason the
-- submitter has already been emailed about.
--
-- Values are the variant names of frontend::project::ProjectRejectionReason
-- ('None', 'Participation', 'Science', 'Spam'). rejection_notified lags
-- rejection_reason until the notice for the current reason has been sent, so
-- a reviewer correcting their choice sends an updated notice while re-saving
-- an unchanged one sends nothing.

ALTER TABLE projects ADD COLUMN rejection_reason TEXT NOT NULL DEFAULT 'None';
ALTER TABLE projects ADD COLUMN rejection_notified TEXT NOT NULL DEFAULT 'None';
