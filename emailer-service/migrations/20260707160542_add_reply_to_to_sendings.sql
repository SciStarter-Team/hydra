-- Optional Reply-To addresses for a send, stored as a JSON array of strings
-- (NULL when unset). Carried on both the sending record and each queued message
-- so the queue processor can apply it at send time, consistent with how
-- template_html / subject / data_sets are duplicated onto send_queue.
ALTER TABLE sendings ADD COLUMN reply_to TEXT;
ALTER TABLE send_queue ADD COLUMN reply_to TEXT;
