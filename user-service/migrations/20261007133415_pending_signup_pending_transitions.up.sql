-- Training steps a tracked go link left waiting in the session of a visitor
-- who had no account yet, as JSON (`common::message::user::PendingTransition`
-- list). Held here for the deferred lane: the completion link may be clicked
-- in another browser, where the session that carried them is gone.
ALTER TABLE "pending_signups" ADD COLUMN "pending_transitions" TEXT NOT NULL DEFAULT '[]';
