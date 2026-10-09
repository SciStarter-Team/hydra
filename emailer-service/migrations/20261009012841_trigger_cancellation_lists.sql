-- A binding may be cancelled by any of several events rather than one: a
-- nudge to start the next step is moot once any of the events that amount to
-- starting it has happened.
CREATE TABLE email_trigger_cancellations (
    trigger_id TEXT NOT NULL REFERENCES email_triggers (trigger_id) ON DELETE CASCADE,
    cancellation_trigger_name TEXT NOT NULL,
    PRIMARY KEY (trigger_id, cancellation_trigger_name)
);

CREATE INDEX idx_email_trigger_cancellations_name
    ON email_trigger_cancellations (cancellation_trigger_name);

INSERT INTO email_trigger_cancellations (trigger_id, cancellation_trigger_name)
SELECT trigger_id, cancellation_trigger_name
  FROM email_triggers
 WHERE cancellation_trigger_name IS NOT NULL
   AND cancellation_trigger_name <> '';

DROP INDEX idx_email_triggers_cancellation;
ALTER TABLE email_triggers DROP COLUMN cancellation_trigger_name;
