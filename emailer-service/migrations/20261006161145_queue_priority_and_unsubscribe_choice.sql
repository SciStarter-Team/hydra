-- Two things a queued message carries about how it goes out, as distinct from
-- what it says and who it goes to.
--
-- `priority`: a message addressed to one person -- a confirmation link, a
-- welcome, a notice to staff -- is something that person is waiting for, and
-- goes out ahead of a mailing to a set, which can take hours to drain. The
-- queue is read highest priority first, and oldest first within a priority.
--
-- `no_unsubscribe`: an email its recipient cannot opt out of, such as the one
-- that asks them to confirm their address, offers no way to unsubscribe: no
-- link in the footer and no one-click header. It is a setting of the stored
-- email, beside its reply-to setting, so that every way of sending the email
-- -- the trigger it is bound to, or a send from the admin to see how it looks
-- -- treats it alike. Each send copies it onto the sending, the scheduled send
-- and the queued message, for the recipients queued later.
ALTER TABLE stored_emails ADD COLUMN no_unsubscribe BOOLEAN NOT NULL DEFAULT 0;
ALTER TABLE send_queue ADD COLUMN priority INTEGER NOT NULL DEFAULT 0;
ALTER TABLE send_queue ADD COLUMN no_unsubscribe BOOLEAN NOT NULL DEFAULT 0;
ALTER TABLE sendings ADD COLUMN no_unsubscribe BOOLEAN NOT NULL DEFAULT 0;
ALTER TABLE scheduled_sends ADD COLUMN no_unsubscribe BOOLEAN NOT NULL DEFAULT 0;

-- The confirmation email seeded by the migration before this one is the email
-- the setting exists for. Named by the id it was seeded with, so that an email
-- staff wrote for the purpose themselves is left as they set it.
UPDATE stored_emails SET no_unsubscribe = 1 WHERE stored_email_id = '01M48Y7CY7CW9JMV2GCTY6025P';

-- The queue is always read by status and then in this order, so the one
-- index serves every reader the old status index did.
DROP INDEX idx_send_queue_status;
CREATE INDEX idx_send_queue_next ON send_queue(status, priority DESC, queue_id);
