-- Add recipients column to email_triggers for admin-configured recipient lists
-- JSON format: ["triggering_user", {"address_set": "slug"}, {"user_set": "slug"}, {"email": "addr"}]
-- NULL defaults to ["triggering_user"] (backward compatible)
ALTER TABLE email_triggers ADD COLUMN recipients TEXT;

-- Add recipients column to scheduled_sends so delayed triggers preserve configured recipients
ALTER TABLE scheduled_sends ADD COLUMN recipients TEXT;
