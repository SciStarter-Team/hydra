-- The project a deferred signup came from, to be joined by the account once
-- the emailed link is clicked (`common::message::user::Signup::join_project`).
ALTER TABLE "pending_signups" ADD COLUMN "join_project" TEXT;
