-- The two emails the People Finder sends, and the triggers that send them.
-- forum-service fires both (forum-service/src/people_finder.rs).
--
--   people-finder-new-request    to the reviewers, when a request is submitted
--   people-finder-confirmation   to the sender, once their message is delivered
--
-- The reviewer notice is orrery's (messaging/api.py, people_near_send), sent
-- to the members of orrery's `people-finder-approvers` list. It is fired with
-- the sender as the triggering user, so its binding names the reviewers
-- itself; staff change who they are in the admin. Its fields arrive already
-- escaped: {{SENDER}}, {{AUDIENCE}}, {{MESSAGE_SUBJECT}}, {{MESSAGE_BODY}} and
-- {{REVIEW_URL}}. Like orrery's, it offers no unsubscribe.
--
-- The confirmation is orrery's `people-finder-confirmation` template (id 95);
-- staff own both from here on.
--
-- Nothing is seeded over a stored email of the same slug, nor a binding of the
-- same event to it, so what staff made first stays.

INSERT INTO stored_emails
    (stored_email_id, slug, subject, body_delta, created_at, updated_at, datasets, reply_to_noreply, no_unsubscribe)
SELECT
    '01M64EBWMEG5CCMC6FR0PJHTAR',
    'people-finder-new-request',
    'Message pending review: People Finder broadcast',
    '{"ops":[{"insert":"A SciStarter People Finder message is awaiting staff review before it can be delivered."},{"insert":"\n"},{"attributes":{"bold":true},"insert":"From:"},{"insert":" {{SENDER}}"},{"insert":"\n"},{"attributes":{"bold":true},"insert":"To:"},{"insert":" {{AUDIENCE}}"},{"insert":"\n"},{"attributes":{"bold":true},"insert":"Subject:"},{"insert":" {{MESSAGE_SUBJECT}}"},{"insert":"\n"},{"insert":"{{MESSAGE_BODY}}"},{"attributes":{"blockquote":true},"insert":"\n"},{"insert":{"button":{"label":"Review this message (approve or deny)","url":"{{REVIEW_URL}}"}}}]}',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    '[]',
    0,
    1
WHERE NOT EXISTS (SELECT 1 FROM stored_emails WHERE slug = 'people-finder-new-request');

INSERT INTO stored_emails
    (stored_email_id, slug, subject, body_delta, created_at, updated_at, datasets, reply_to_noreply, no_unsubscribe)
SELECT
    '01M64EBWMEENKQDHS8VJF5S9J6',
    'people-finder-confirmation',
    'The message you created through SciStarter''s People Finder has been approved and sent to your recipients!',
    '{"ops":[{"insert":"Your message has been sent to the people in your target area."},{"insert":"\n"},{"insert":"The People Finder tool is still in beta. Will you "},{"attributes":{"link":"https://scistarter.org/feedback#people-finder"},"insert":"leave feedback"},{"insert":" on your experience with the People Finder tool?"},{"insert":"\n"},{"insert":"Thank you."},{"insert":"\n"},{"insert":"The SciStarter Team"},{"insert":"\n"}]}',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    '[]',
    0,
    0
WHERE NOT EXISTS (SELECT 1 FROM stored_emails WHERE slug = 'people-finder-confirmation');

INSERT INTO email_triggers
    (trigger_id, trigger_name, stored_email_id, action_type, recipients, active, created_at, updated_at)
SELECT
    '01M64EBWME61HWFPCCT4BDTXSX',
    'people-finder-new-request',
    e.stored_email_id,
    'immediate',
    '[{"email": "caroline@scistarter.org"}, {"email": "roland@scistarter.org"}, {"email": "emma@scistarter.org"}, {"email": "daniel@scistarter.org"}, {"email": "darlene@scistarter.org"}]',
    1,
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
  FROM stored_emails e
 WHERE e.slug = 'people-finder-new-request'
   AND NOT EXISTS (
           SELECT 1 FROM email_triggers t JOIN stored_emails s ON s.stored_email_id = t.stored_email_id
            WHERE t.trigger_name = 'people-finder-new-request' AND s.slug = 'people-finder-new-request'
       );

INSERT INTO email_triggers
    (trigger_id, trigger_name, stored_email_id, action_type, active, created_at, updated_at)
SELECT
    '01M64EBWMER895DC1THYYW3X36',
    'people-finder-confirmation',
    e.stored_email_id,
    'immediate',
    1,
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
  FROM stored_emails e
 WHERE e.slug = 'people-finder-confirmation'
   AND NOT EXISTS (
           SELECT 1 FROM email_triggers t JOIN stored_emails s ON s.stored_email_id = t.stored_email_id
            WHERE t.trigger_name = 'people-finder-confirmation' AND s.slug = 'people-finder-confirmation'
       );
