-- Where people get tickets or register, when that isn't the event's own
-- link, and what it costs to attend, in the organizer's words ("Free",
-- "$10 suggested donation").
ALTER TABLE "events" ADD COLUMN "ticket_url" TEXT NOT NULL DEFAULT '';
ALTER TABLE "events" ADD COLUMN "cost" TEXT NOT NULL DEFAULT '';

-- An event held at its address that can also be joined online, at its
-- `url`. Never set alongside `entirely_online`: an event joined only online
-- is that instead.
ALTER TABLE "events" ADD COLUMN "also_online" BOOLEAN NOT NULL DEFAULT FALSE;
