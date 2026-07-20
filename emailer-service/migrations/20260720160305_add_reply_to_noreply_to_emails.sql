-- When set, replies to this email are directed to the no-reply mailbox instead
-- of the send-from address. Stored on the email record rather than on the send
-- so that trigger-driven sends inherit it along with the subject and body.
ALTER TABLE stored_emails ADD COLUMN reply_to_noreply BOOLEAN NOT NULL DEFAULT 0;
ALTER TABLE oneoff_emails ADD COLUMN reply_to_noreply BOOLEAN NOT NULL DEFAULT 0;

-- Delayed trigger sends are parked here before being handed to the send queue,
-- so the resolved Reply-To has to survive the wait. Same JSON-array-of-strings
-- encoding (NULL when unset) used by sendings.reply_to and send_queue.reply_to.
ALTER TABLE scheduled_sends ADD COLUMN reply_to TEXT;
