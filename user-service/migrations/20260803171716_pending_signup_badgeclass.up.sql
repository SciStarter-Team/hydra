-- A badge staked out by the tracked go link the visitor arrived through.
-- Held here for the deferred lane: the completion link may be clicked in
-- another browser, where the session key that carried it is gone.
ALTER TABLE "pending_signups" ADD COLUMN "badgeclass" TEXT;
