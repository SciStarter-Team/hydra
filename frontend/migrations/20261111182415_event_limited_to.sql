-- The group an event is limited to, in the organizer's words ("Library
-- cardholders", "K-12 teachers"); empty when it's open to everyone.
ALTER TABLE "events" ADD COLUMN "limited_to" TEXT NOT NULL DEFAULT '';
