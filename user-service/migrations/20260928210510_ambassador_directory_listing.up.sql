-- The public face of the Ambassador directory.
--
-- `ambassador_directory_members` answers "is this person an Ambassador?", and
-- certificates ask it too. Whether the directory *shows* them is a narrower
-- question: staff can hide an application from the listing without taking the
-- role away. The directory, a profile's public view, its nearby strip, the map
-- and the contact form all ask this narrower one, and read the application's
-- public fields through it, so none of them can disagree about who is listed
-- or what the listing says. An Ambassador who predates the application form
-- has no application row and is listed with empty fields.
CREATE VIEW "ambassador_directory_listing" AS
    SELECT m."user_id" AS "user_id",
           COALESCE(a."city", '') AS "city",
           COALESCE(a."state", '') AS "state",
           COALESCE(a."bio", '') AS "bio",
           COALESCE(a."youtube_video_url", '') AS "youtube_video_url",
           a."position" AS "position"
      FROM "ambassador_directory_members" m
      LEFT JOIN "ambassador_applications" a ON a."user_id" = m."user_id"
     WHERE COALESCE(a."hidden", 0) = 0;

-- The directory's default order: a random draw per application, made once and
-- kept, so the order is a shuffle that holds still between pages and visits.
-- orrery draws its own (`AmbassadorApplication.position`) and the sync carries
-- it; the trigger draws one for any row that arrives without, whichever path
-- inserted it, so no insert has to remember to.
ALTER TABLE "ambassador_applications" ADD COLUMN "position" REAL;

UPDATE "ambassador_applications"
   SET "position" = random() / 18446744073709551616.0 + 0.5;

CREATE TRIGGER "ambassador_applications_draw_position"
    AFTER INSERT ON "ambassador_applications"
    WHEN NEW."position" IS NULL
BEGIN
    UPDATE "ambassador_applications"
       SET "position" = random() / 18446744073709551616.0 + 0.5
     WHERE "user_id" = NEW."user_id";
END;

-- Projects and events a person is linked to as an ambassador (orrery's
-- `User.ambassador_connections`). Staff set these by hand, and orrery adds
-- the owner of an ambassador-tagged event automatically. The profile shows the
-- linked events alongside the ones the person manages.
CREATE TABLE "ambassador_connections" (
    "user_id" text NOT NULL REFERENCES "users" ("user_id") ON DELETE CASCADE,
    "entity_id" text NOT NULL,
    PRIMARY KEY ("user_id", "entity_id")
);
