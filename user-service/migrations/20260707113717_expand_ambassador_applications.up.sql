-- Expand ambassador_applications from the initial self-edit stub to the full
-- orrery AmbassadorApplication field set. This backs the public apply form and
-- the staff processing spreadsheet. Fields mirror orrery's core.models
-- AmbassadorApplication (7523+); orrery's `email` is our existing
-- `contact_email`, `updated` is our `updated_at`. The `Withdrawn` app_status is
-- intentionally omitted (retired in orrery in favor of demoting to Champion).

-- Review / workflow flags (staff-editable in the spreadsheet)
ALTER TABLE "ambassador_applications" ADD COLUMN "hidden" integer NOT NULL DEFAULT 0;
ALTER TABLE "ambassador_applications" ADD COLUMN "marked" integer NOT NULL DEFAULT 0;
ALTER TABLE "ambassador_applications" ADD COLUMN "archived" integer NOT NULL DEFAULT 0;
ALTER TABLE "ambassador_applications" ADD COLUMN "draft" integer NOT NULL DEFAULT 1;
ALTER TABLE "ambassador_applications" ADD COLUMN "cohort" integer NOT NULL DEFAULT 5;
ALTER TABLE "ambassador_applications" ADD COLUMN "reviewers" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "notes" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "screening_status" integer NOT NULL DEFAULT 0;
ALTER TABLE "ambassador_applications" ADD COLUMN "verified" text;
ALTER TABLE "ambassador_applications" ADD COLUMN "welcome_sent" integer NOT NULL DEFAULT 0;
ALTER TABLE "ambassador_applications" ADD COLUMN "rejection_sent" integer NOT NULL DEFAULT 0;
ALTER TABLE "ambassador_applications" ADD COLUMN "orientation" text;
ALTER TABLE "ambassador_applications" ADD COLUMN "kit_sent" text;
ALTER TABLE "ambassador_applications" ADD COLUMN "survey" integer NOT NULL DEFAULT 0;

-- Applicant demographic / contact fields
ALTER TABLE "ambassador_applications" ADD COLUMN "birthdate" text;
ALTER TABLE "ambassador_applications" ADD COLUMN "phone" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "tee_size" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "gender" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "ethnicity" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "handles" text NOT NULL DEFAULT '';

-- Applicant essay / free-text fields
ALTER TABLE "ambassador_applications" ADD COLUMN "self_description" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "introduction" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "introduction_other" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "experience" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "motivation" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "related" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "vision" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "activities" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "community_access" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "questions" text NOT NULL DEFAULT '';

-- Agreements / signature
ALTER TABLE "ambassador_applications" ADD COLUMN "citscimonth" integer NOT NULL DEFAULT 0;
ALTER TABLE "ambassador_applications" ADD COLUMN "background_check" integer NOT NULL DEFAULT 0;
ALTER TABLE "ambassador_applications" ADD COLUMN "conduct" integer NOT NULL DEFAULT 0;
ALTER TABLE "ambassador_applications" ADD COLUMN "signature_name" text NOT NULL DEFAULT '';
ALTER TABLE "ambassador_applications" ADD COLUMN "signature_date" text;

CREATE INDEX "ambassador_applications_by_cohort" ON "ambassador_applications" ("cohort");
CREATE INDEX "ambassador_applications_by_screening_status" ON "ambassador_applications" ("screening_status");
