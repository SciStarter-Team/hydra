-- Staff notices about new projects go to the shared projects inbox, as
-- orrery's project-reviewers list does since emailer migration 0036.
--
-- These triggers are fired with the submitter as the triggering user, so a
-- binding left with no recipients of its own would mail the submitter
-- instead of staff. Bindings staff create after this need the same
-- recipients, set in the admin.

UPDATE email_triggers
SET recipients = '[{"email": "projects@scistarter.org"}]',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE trigger_name IN ('project-created-staff', 'project-created-api');
