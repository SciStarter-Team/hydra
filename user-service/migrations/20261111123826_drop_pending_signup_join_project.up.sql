-- A signup no longer joins the project it was made from: signing up on a
-- project's page says nothing about taking part in the project.
ALTER TABLE "pending_signups" DROP COLUMN "join_project";
