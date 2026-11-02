-- A project's How-To Video is `video_url` and the `video_label` that words its
-- link. A value held only by the retired `howto_video` moves to `video_url`.
UPDATE projects
SET video_url = howto_video
WHERE video_url = '' AND howto_video <> '';

ALTER TABLE projects DROP COLUMN howto_video;

ALTER TABLE projects ADD COLUMN video_label text not null default '';

DELETE FROM project_field_sources WHERE field_type = 'howto_video';

-- The label answers to the same authority as the video it words, so an import
-- may not rename a video that an import may not replace.
INSERT INTO project_field_sources (project_id, field_type, source)
SELECT project_id, 'video_label', source
FROM project_field_sources
WHERE field_type = 'video_url'
ON CONFLICT (project_id, field_type) DO NOTHING;
