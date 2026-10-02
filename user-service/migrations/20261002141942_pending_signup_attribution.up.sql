-- Where the visitor came from (their first page load's referrer, landing
-- path, and UTM tags), held here for the deferred lane: the completion link
-- may be clicked in another browser, where the session that carried it is
-- gone. Copied into the same columns of "users" when the account is created.
ALTER TABLE "pending_signups" ADD COLUMN "referer" TEXT NOT NULL DEFAULT '';
ALTER TABLE "pending_signups" ADD COLUMN "origin" TEXT NOT NULL DEFAULT '';
ALTER TABLE "pending_signups" ADD COLUMN "utm_source" TEXT NOT NULL DEFAULT '';
ALTER TABLE "pending_signups" ADD COLUMN "utm_medium" TEXT NOT NULL DEFAULT '';
ALTER TABLE "pending_signups" ADD COLUMN "utm_campaign" TEXT NOT NULL DEFAULT '';
ALTER TABLE "pending_signups" ADD COLUMN "utm_term" TEXT NOT NULL DEFAULT '';
ALTER TABLE "pending_signups" ADD COLUMN "utm_content" TEXT NOT NULL DEFAULT '';
