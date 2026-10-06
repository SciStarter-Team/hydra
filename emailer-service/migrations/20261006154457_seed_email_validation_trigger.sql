-- The email that asks a new member to confirm their address, and the trigger
-- that sends it. The user service fires `email-validation` whenever an account
-- needs confirming, and nothing reaches the member unless a trigger of that
-- name is bound to an email -- so the pair is seeded rather than left for
-- somebody to remember to create.
--
-- Staff own the wording from here on: the email is edited like any other
-- stored email. `{{VERIFY_URL}}` is the confirmation link, which the button
-- leads to, and `{{VERIFY_CODE}}` the code alone, which is what the template
-- orrery sends is written in terms of.
--
-- Nothing is seeded where a trigger of this name already exists, so a
-- database on which staff got here first keeps what they made.

INSERT OR IGNORE INTO stored_emails
    (stored_email_id, slug, subject, body_delta, created_at, updated_at, datasets, reply_to_noreply)
SELECT
    '01M48Y7CY7CW9JMV2GCTY6025P',
    'email-validation',
    'Welcome to SciStarter — confirm your email',
    '{"ops":[{"insert":"Welcome to SciStarter!"},{"attributes":{"header":2},"insert":"\n"},{"attributes":{"bold":true},"insert":"You’re one click away from getting started."},{"insert":"\nThanks for joining SciStarter, where millions of people discover thousands of opportunities to participate in Acts of Science to help real research!\nConfirm your email to unlock more of SciStarter—connect with other members and project scientists, earn and share digital badges, and more.\n"},{"insert":{"button":{"url":"{{VERIFY_URL}}","label":"CONFIRM MY EMAIL"}}},{"attributes":{"italic":true},"insert":"If you didn’t create a SciStarter account, you can safely ignore this email."},{"insert":"\n"}]}',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    '[]',
    0
WHERE NOT EXISTS (SELECT 1 FROM email_triggers WHERE trigger_name = 'email-validation');

INSERT INTO email_triggers
    (trigger_id, trigger_name, stored_email_id, action_type, active, created_at, updated_at)
SELECT
    '01M48Y7CYH3MRYS5K7WMAYRVQN',
    'email-validation',
    stored_email_id,
    'immediate',
    1,
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM stored_emails
WHERE slug = 'email-validation'
  AND NOT EXISTS (SELECT 1 FROM email_triggers WHERE trigger_name = 'email-validation');
