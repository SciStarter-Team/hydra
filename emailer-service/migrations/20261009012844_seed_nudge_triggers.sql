-- The timed nudges that move a member along to their next step, ported from
-- orrery (orrery/emailer/nudges.py), and the delayed triggers that send them.
--
-- Each is a delayed binding: the anchor event schedules the email, and any of
-- its cancelling events -- the step the email asks for, or one that makes it
-- moot -- cancels it or stops it being scheduled. orrery's sync replays its
-- history as these same events (TriggerEmailRequest::occurred_at), so the
-- sends a member is owed are already waiting when hydra becomes the primary.
--
--   foundations-invitation          7 days after signing up (any welcome), unless Foundations is begun
--   champion-training-invitation    7 days after the Foundations badge, unless the Champion training is done
--   champion-training-complete-...  2 days after the Champion training, unless they apply or are promoted
--   champion-ambassador-followup   30 days after the Champion training, the same
--   ambassador-onboarding-reminder 30 days after acceptance, unless orientation is recorded, or they are promoted or demoted
--
-- The copy is orrery's (emailer migration 0034, and staff's template for the
-- two-day reminder); staff own it from here on. orrery's Foundations badge
-- nudge has no counterpart: nobody can stop part-way through Foundations here.
--
-- Nothing is seeded over a stored email of the same slug, nor a binding of the
-- same event to it, so what staff made first stays.

INSERT INTO stored_emails
    (stored_email_id, slug, subject, body_delta, created_at, updated_at, datasets, reply_to_noreply)
SELECT
    '01M4F52CC0G35E3AY36E1YVEW4',
    'foundations-invitation',
    'Ready to build your citizen science skills?',
    '{"ops":[{"insert":"Hi {{FRIENDLY_NAME}},"},{"insert":"\n"},{"insert":"You''ve joined SciStarter and explored what citizen science has to offer. Ready to learn a little more?"},{"insert":"\n"},{"insert":"Our free "},{"attributes":{"link":"https://scistarter.org/training-foundations?utm_campaign=UM-foundations-invitation&utm_medium=email&utm_content=training"},"insert":"Foundations of Citizen Science training"},{"insert":" is a great place to start! You''ll learn more about what citizen science is, how to find projects that interest you, and how your participation can contribute to real scientific research."},{"insert":"\n"},{"insert":"The training is self-paced, so you can complete it whenever works for you. Once you finish, you''ll earn a digital badge recognizing your new skills!"},{"insert":"\n"},{"insert":{"button":{"label":"Start Foundations of Citizen Science","url":"https://scistarter.org/training-foundations?utm_campaign=UM-foundations-invitation&utm_medium=email&utm_content=cta"}}},{"insert":"We hope you''ll join us!"},{"insert":"\n"},{"insert":"The SciStarter Team"},{"insert":"\n"}]}',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    '["user_identity"]',
    0
WHERE NOT EXISTS (SELECT 1 FROM stored_emails WHERE slug = 'foundations-invitation');

INSERT INTO stored_emails
    (stored_email_id, slug, subject, body_delta, created_at, updated_at, datasets, reply_to_noreply)
SELECT
    '01M4F52CC0ZMGYYPSP3069QBES',
    'champion-training-invitation',
    'Take the next step: become a citizen science champion',
    '{"ops":[{"insert":"Hi {{FRIENDLY_NAME}},"},{"insert":"\n"},{"insert":"Congratulations again on completing Foundations of Citizen Science!"},{"insert":"\n"},{"insert":"If you''re interested in sharing citizen science with others, we''d love to invite you to take the next step and become a Citizen Science Champion."},{"insert":"\n"},{"insert":"Our free "},{"attributes":{"link":"https://scistarter.org/training-champion?utm_campaign=UM-champion-training-invitation&utm_medium=email&utm_content=training"},"insert":"Champion training"},{"insert":" builds on what you''ve already learned and introduces resources and ideas for helping friends, family, and your community get involved in citizen science."},{"insert":"\n"},{"insert":"Once you complete the training, you''ll earn a Champion certificate and gain access to additional resources, opportunities, and a community of fellow Champions."},{"insert":"\n"},{"insert":{"button":{"label":"Explore the Citizen Science Champion Training","url":"https://scistarter.org/training-champion?utm_campaign=UM-champion-training-invitation&utm_medium=email&utm_content=cta"}}},{"insert":"Whether you''re sharing a favorite project with a friend or introducing citizen science to a larger group, there''s a place for you in the Champion Network!"},{"insert":"\n"},{"insert":"The SciStarter Team"},{"insert":"\n"}]}',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    '["user_identity"]',
    0
WHERE NOT EXISTS (SELECT 1 FROM stored_emails WHERE slug = 'champion-training-invitation');

INSERT INTO stored_emails
    (stored_email_id, slug, subject, body_delta, created_at, updated_at, datasets, reply_to_noreply)
SELECT
    '01M4F52CC0R5WCMQSHM74E70G6',
    'champion-training-complete-reminder-2daysafter',
    'Reminder: Invitation to Apply to Become a SciStarter Ambassador!',
    '{"ops":[{"insert":"Hello {{FRIENDLY_NAME}},"},{"insert":"\n"},{"insert":"Congratulations again on completing the Citizen Science Champion training!"},{"insert":"\n"},{"insert":"As a Champion, you’re better prepared to talk about Citizen Science and SciStarter with friends, family and others in your own networks. And you have access to a "},{"insert":"Champion Handbook","attributes":{"link":"http://scistarter.org/go/Champion-Handbook"}},{"insert":", a "},{"insert":"peer network on Facebook","attributes":{"link":"https://www.facebook.com/share/g/18ohvpCGdy/"}},{"insert":", and"},{"insert":" other benefits and resources","attributes":{"link":"http://scistarter.org/champion-resources"}},{"insert":". "},{"insert":"\n"},{"insert":"If you are 18 years or older, and reside in the U.S., you’re also eligible to receive additional training and resources to expand your role as a SciStarter Ambassador!"},{"insert":"\n"},{"insert":"SciStarter Ambassadors are volunteers who receive additional training, resources, and other benefits; appear in a "},{"insert":"public directory ","attributes":{"link":"https://scistarter.org/ambassadors-directory"}},{"insert":"on SciStarter; and support libraries, museums, schools, and other community organizations interested in introducing citizen science and SciStarter to their audiences. "},{"insert":"\n"},{"insert":"Interested? Apply to become a SciStarter Ambassador: "},{"insert":"https://scistarter.org/ambassador-apply","attributes":{"link":"https://scistarter.org/ambassadors-apply"}},{"insert":"\n"},{"insert":"Email "},{"insert":"info@SciStarter.org","attributes":{"link":"mailto:info@SciStarter.org"}},{"insert":" with any questions. "},{"insert":"\n"},{"insert":"Cheers!"},{"insert":"\n"},{"insert":"The SciStarter Team"},{"insert":"\n"}]}',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    '["user_identity"]',
    0
WHERE NOT EXISTS (SELECT 1 FROM stored_emails WHERE slug = 'champion-training-complete-reminder-2daysafter');

INSERT INTO stored_emails
    (stored_email_id, slug, subject, body_delta, created_at, updated_at, datasets, reply_to_noreply)
SELECT
    '01M4F52CC0D162TF8X1MHKFQG7',
    'champion-ambassador-followup',
    'Interested in taking your Champion role further?',
    '{"ops":[{"insert":"Hi {{FRIENDLY_NAME}},"},{"insert":"\n"},{"insert":"It''s been a little while since you became a SciStarter Citizen Science Champion, and we wanted to check in!"},{"insert":"\n"},{"insert":"Have you had a chance to share citizen science with someone in your community? Whether you''ve recommended a project to a friend, participated in an event, or introduced someone to SciStarter, we''d love to hear about it."},{"insert":"\n"},{"attributes":{"bold":true},"insert":"Interested in doing even more?"},{"insert":"\n"},{"insert":"Consider applying to become a SciStarter Ambassador! Ambassadors receive additional training, resources, and support to help bring citizen science to libraries, schools, museums, and other community spaces."},{"insert":"\n"},{"insert":"If you''re 18 or older and live in the United States, you may be eligible to apply."},{"insert":"\n"},{"insert":{"button":{"label":"Learn more about becoming an Ambassador","url":"https://scistarter.org/Ambassador?utm_campaign=UM-champion-ambassador-followup&utm_medium=email&utm_content=cta"}}},{"insert":"Not ready to become an Ambassador? That''s completely fine! You can continue making a difference as a Champion by sharing projects, connecting with others in our "},{"attributes":{"link":"https://www.facebook.com/share/g/1EasLbw1Do/?utm_campaign=UM-champion-ambassador-followup&utm_medium=email&utm_content=facebook"},"insert":"Facebook group"},{"insert":", or exploring new opportunities on "},{"attributes":{"link":"https://scistarter.org/finder?utm_campaign=UM-champion-ambassador-followup&utm_medium=email&utm_content=finder"},"insert":"SciStarter"},{"insert":"."},{"insert":"\n"},{"insert":"Thanks for being part of our community!"},{"insert":"\n"},{"insert":"The SciStarter Team"},{"insert":"\n"}]}',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    '["user_identity"]',
    0
WHERE NOT EXISTS (SELECT 1 FROM stored_emails WHERE slug = 'champion-ambassador-followup');

INSERT INTO stored_emails
    (stored_email_id, slug, subject, body_delta, created_at, updated_at, datasets, reply_to_noreply)
SELECT
    '01M4F52CC0QBRDVTB9DAKG6YMK',
    'ambassador-onboarding-reminder',
    'Only a few steps left to complete your Ambassador onboarding',
    '{"ops":[{"insert":"Hi {{FRIENDLY_NAME}},"},{"insert":"\n"},{"insert":"We''re excited to have you in the SciStarter Ambassador Network!"},{"insert":"\n"},{"insert":"We noticed that you haven''t attended an Ambassador Orientation yet, and we wanted to help you get started."},{"insert":"\n"},{"insert":"All new Ambassadors are required to attend one live, virtual orientation. During the session, we''ll introduce you to the program, walk through available resources, answer questions, and help you prepare to share citizen science with your community."},{"insert":"\n"},{"insert":{"button":{"label":"Register for an Ambassador Orientation","url":"https://scistarter.org/go/ambassador-orientation?utm_campaign=UM-ambassador-onboarding-reminder&utm_medium=email&utm_content=cta"}}},{"insert":"If you''ve already registered for an upcoming session, you''re all set! We look forward to seeing you there."},{"insert":"\n"},{"insert":"The SciStarter Team"},{"insert":"\n"}]}',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    '["user_identity"]',
    0
WHERE NOT EXISTS (SELECT 1 FROM stored_emails WHERE slug = 'ambassador-onboarding-reminder');

INSERT INTO email_triggers
    (trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, active, created_at, updated_at)
SELECT '01M4F52CC0EK5ND010VG4V55ZJ', 'user-welcome', e.stored_email_id, 'delayed', 604800, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
  FROM stored_emails e
 WHERE e.slug = 'foundations-invitation'
   AND NOT EXISTS (
           SELECT 1 FROM email_triggers t JOIN stored_emails s ON s.stored_email_id = t.stored_email_id
            WHERE t.trigger_name = 'user-welcome' AND s.slug = 'foundations-invitation'
       );

INSERT OR IGNORE INTO email_trigger_cancellations (trigger_id, cancellation_trigger_name)
SELECT t.trigger_id, c.name
  FROM email_triggers t
  JOIN (SELECT 'foundations-tutorial' AS name UNION ALL SELECT 'foundations-squirrel' AS name UNION ALL SELECT 'foundations-stall' AS name UNION ALL SELECT 'foundations-debris' AS name UNION ALL SELECT 'badge-foundations' AS name) AS c
 WHERE t.trigger_id = '01M4F52CC0EK5ND010VG4V55ZJ';

INSERT INTO email_triggers
    (trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, active, created_at, updated_at)
SELECT '01M4F52CC0MFE62DDFY0BA0R60', 'user-welcome-girl-scout', e.stored_email_id, 'delayed', 604800, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
  FROM stored_emails e
 WHERE e.slug = 'foundations-invitation'
   AND NOT EXISTS (
           SELECT 1 FROM email_triggers t JOIN stored_emails s ON s.stored_email_id = t.stored_email_id
            WHERE t.trigger_name = 'user-welcome-girl-scout' AND s.slug = 'foundations-invitation'
       );

INSERT OR IGNORE INTO email_trigger_cancellations (trigger_id, cancellation_trigger_name)
SELECT t.trigger_id, c.name
  FROM email_triggers t
  JOIN (SELECT 'foundations-tutorial' AS name UNION ALL SELECT 'foundations-squirrel' AS name UNION ALL SELECT 'foundations-stall' AS name UNION ALL SELECT 'foundations-debris' AS name UNION ALL SELECT 'badge-foundations' AS name) AS c
 WHERE t.trigger_id = '01M4F52CC0MFE62DDFY0BA0R60';

INSERT INTO email_triggers
    (trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, active, created_at, updated_at)
SELECT '01M4F52CC0TW6HE4AN9FVTB27R', 'user-welcome-girl-scout-volunteer', e.stored_email_id, 'delayed', 604800, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
  FROM stored_emails e
 WHERE e.slug = 'foundations-invitation'
   AND NOT EXISTS (
           SELECT 1 FROM email_triggers t JOIN stored_emails s ON s.stored_email_id = t.stored_email_id
            WHERE t.trigger_name = 'user-welcome-girl-scout-volunteer' AND s.slug = 'foundations-invitation'
       );

INSERT OR IGNORE INTO email_trigger_cancellations (trigger_id, cancellation_trigger_name)
SELECT t.trigger_id, c.name
  FROM email_triggers t
  JOIN (SELECT 'foundations-tutorial' AS name UNION ALL SELECT 'foundations-squirrel' AS name UNION ALL SELECT 'foundations-stall' AS name UNION ALL SELECT 'foundations-debris' AS name UNION ALL SELECT 'badge-foundations' AS name) AS c
 WHERE t.trigger_id = '01M4F52CC0TW6HE4AN9FVTB27R';

INSERT INTO email_triggers
    (trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, active, created_at, updated_at)
SELECT '01M4F52CC0RZND6979XXQTJ8ZK', 'badge-foundations', e.stored_email_id, 'delayed', 604800, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
  FROM stored_emails e
 WHERE e.slug = 'champion-training-invitation'
   AND NOT EXISTS (
           SELECT 1 FROM email_triggers t JOIN stored_emails s ON s.stored_email_id = t.stored_email_id
            WHERE t.trigger_name = 'badge-foundations' AND s.slug = 'champion-training-invitation'
       );

INSERT OR IGNORE INTO email_trigger_cancellations (trigger_id, cancellation_trigger_name)
SELECT t.trigger_id, c.name
  FROM email_triggers t
  JOIN (SELECT 'ambassador-training-complete' AS name) AS c
 WHERE t.trigger_id = '01M4F52CC0RZND6979XXQTJ8ZK';

INSERT INTO email_triggers
    (trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, active, created_at, updated_at)
SELECT '01M4F52CC0PWBHQRJ3B329SGYA', 'ambassador-training-complete', e.stored_email_id, 'delayed', 172800, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
  FROM stored_emails e
 WHERE e.slug = 'champion-training-complete-reminder-2daysafter'
   AND NOT EXISTS (
           SELECT 1 FROM email_triggers t JOIN stored_emails s ON s.stored_email_id = t.stored_email_id
            WHERE t.trigger_name = 'ambassador-training-complete' AND s.slug = 'champion-training-complete-reminder-2daysafter'
       );

INSERT OR IGNORE INTO email_trigger_cancellations (trigger_id, cancellation_trigger_name)
SELECT t.trigger_id, c.name
  FROM email_triggers t
  JOIN (SELECT 'ambassador-application-submitted' AS name UNION ALL SELECT 'ambassador-promoted' AS name) AS c
 WHERE t.trigger_id = '01M4F52CC0PWBHQRJ3B329SGYA';

INSERT INTO email_triggers
    (trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, active, created_at, updated_at)
SELECT '01M4F52CC03J5W19CS4CA2DTV9', 'ambassador-training-complete', e.stored_email_id, 'delayed', 2592000, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
  FROM stored_emails e
 WHERE e.slug = 'champion-ambassador-followup'
   AND NOT EXISTS (
           SELECT 1 FROM email_triggers t JOIN stored_emails s ON s.stored_email_id = t.stored_email_id
            WHERE t.trigger_name = 'ambassador-training-complete' AND s.slug = 'champion-ambassador-followup'
       );

INSERT OR IGNORE INTO email_trigger_cancellations (trigger_id, cancellation_trigger_name)
SELECT t.trigger_id, c.name
  FROM email_triggers t
  JOIN (SELECT 'ambassador-application-submitted' AS name UNION ALL SELECT 'ambassador-promoted' AS name) AS c
 WHERE t.trigger_id = '01M4F52CC03J5W19CS4CA2DTV9';

INSERT INTO email_triggers
    (trigger_id, trigger_name, stored_email_id, action_type, delay_seconds, active, created_at, updated_at)
SELECT '01M4F52CC0VXZ53RSDTK2BNVN4', 'badge-ambassador', e.stored_email_id, 'delayed', 2592000, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
  FROM stored_emails e
 WHERE e.slug = 'ambassador-onboarding-reminder'
   AND NOT EXISTS (
           SELECT 1 FROM email_triggers t JOIN stored_emails s ON s.stored_email_id = t.stored_email_id
            WHERE t.trigger_name = 'badge-ambassador' AND s.slug = 'ambassador-onboarding-reminder'
       );

INSERT OR IGNORE INTO email_trigger_cancellations (trigger_id, cancellation_trigger_name)
SELECT t.trigger_id, c.name
  FROM email_triggers t
  JOIN (SELECT 'ambassador-orientation-recorded' AS name UNION ALL SELECT 'ambassador-promoted' AS name UNION ALL SELECT 'ambassador-demoted' AS name) AS c
 WHERE t.trigger_id = '01M4F52CC0VXZ53RSDTK2BNVN4';

