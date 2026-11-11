-- Orrery's participation tools ("SciStarter Login Project" one-click signup)
-- are deprecated, and hydra does not carry them.

DELETE FROM project_field_sources
 WHERE field_type IN (
       'participation_tools',
       'participation_prompt',
       'participation_confirmation_message',
       'participation_notification_email',
       'participation_usage'
 );

ALTER TABLE projects DROP COLUMN participation_tools;
ALTER TABLE projects DROP COLUMN participation_prompt;
ALTER TABLE projects DROP COLUMN participation_confirmation_message;
ALTER TABLE projects DROP COLUMN participation_notification_email;
ALTER TABLE projects DROP COLUMN participation_usage;
