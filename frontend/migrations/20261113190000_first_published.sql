-- When each project and event was first made public, as orrery keeps it in
-- `published_at`. A submitter is told their submission is published only
-- the first time it is, however often its page is unpublished and published
-- again; NULL means it has never been public.
ALTER TABLE projects ADD COLUMN published_at TEXT;
ALTER TABLE events ADD COLUMN published_at TEXT;

-- What is public already has been told, or was public before hydra kept
-- this. When it first went public is not known here, only that it was no
-- earlier than its creation, so creation stands in until the orrery sync
-- next carries the project or event and brings orrery's own record.
UPDATE projects SET published_at = created
 WHERE published_at IS NULL
   AND project_id IN (SELECT target FROM entities WHERE kind = 'Project' AND publish);

UPDATE events SET published_at = created
 WHERE published_at IS NULL
   AND event_id IN (SELECT target FROM entities WHERE kind = 'Event' AND publish);
