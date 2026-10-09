-- A delayed send is held under the binding that scheduled it, as
-- 'trigger:<trigger_id>', rather than under the event that fired it. Keyed by
-- event, cancelling one binding's send cancelled every delayed send on the
-- same event; keyed by binding, each is cancelled by its own events only.
--
-- One pending send per user and hook: firing an event again, or the sync
-- replaying it, leaves the send already waiting where it is.

-- Re-key the sends already waiting, where the stored email's subject says
-- which of the event's delayed bindings scheduled them. A send that cannot be
-- placed keeps its event key, which cancellation still honours.
UPDATE scheduled_sends
   SET hook = 'trigger:' || (
           SELECT t.trigger_id
             FROM email_triggers t
             JOIN stored_emails e ON e.stored_email_id = t.stored_email_id
            WHERE 'trigger:' || t.trigger_name = scheduled_sends.hook
              AND t.action_type = 'delayed'
              AND e.subject = scheduled_sends.subject
       )
 WHERE hook LIKE 'trigger:%'
   AND (
           SELECT count(*)
             FROM email_triggers t
             JOIN stored_emails e ON e.stored_email_id = t.stored_email_id
            WHERE 'trigger:' || t.trigger_name = scheduled_sends.hook
              AND t.action_type = 'delayed'
              AND e.subject = scheduled_sends.subject
       ) = 1;

-- Duplicates are the same email waiting twice for the same person. Keep the
-- first scheduled (a scheduled_id is a ULID, so it sorts by creation).
DELETE FROM scheduled_sends
 WHERE scheduled_id NOT IN (
           SELECT min(scheduled_id) FROM scheduled_sends GROUP BY user_id, hook
       );

DROP INDEX idx_scheduled_sends_user_hook;
CREATE UNIQUE INDEX idx_scheduled_sends_user_hook ON scheduled_sends (user_id, hook);
