-- Forms SciStarter hosts for projects, and what people submit through them
-- (common's `message::form`). Until cutover both are copies of orrery's
-- ProjectForm and ProjectFormData, carrying orrery's ids.

CREATE TABLE "forms" (
    "form_id" TEXT PRIMARY KEY NOT NULL,
    -- orrery's id, which old `/form/<id>` links name. NULL for a form made
    -- here.
    "orrery_id" INTEGER UNIQUE,
    "slug" TEXT UNIQUE,
    "name" TEXT NOT NULL,
    -- Markdown shown above the fields, and beside them.
    "intro" TEXT NOT NULL DEFAULT '',
    "aside" TEXT NOT NULL DEFAULT '',
    "image" TEXT NOT NULL DEFAULT '',
    -- JSON: the form's fields, in order.
    "fields" TEXT NOT NULL DEFAULT '[]',
    "closed" INTEGER NOT NULL DEFAULT 0,
    "sign_in_required" INTEGER NOT NULL DEFAULT 1,
    -- Whether submissions are shown to others only without who made them
    -- or exactly where.
    "anonymized" INTEGER NOT NULL DEFAULT 0,
    -- Whether a submission credits its submitter with taking part in the
    -- project.
    "credits_participation" INTEGER NOT NULL DEFAULT 1,
    -- The field whose number says how many contributions a submission is.
    "repetitions_field" TEXT,
    "next_url" TEXT NOT NULL DEFAULT '',
    -- HTML shown once a submission is saved.
    "confirmation_message" TEXT NOT NULL DEFAULT '',
    -- The slug of the stored email sent on someone's first submission.
    "first_submission_email" TEXT,
    "project_id" TEXT,
    "project_role" TEXT CHECK ("project_role" IN ('main', 'extra')),
    -- JSON: the public map of its submissions, or NULL for none.
    "map" TEXT,
    "created" TEXT NOT NULL,
    "updated" TEXT NOT NULL,
    CHECK (("project_id" IS NULL) = ("project_role" IS NULL))
);

-- A project has at most one main form.
CREATE UNIQUE INDEX "forms_main_by_project" ON "forms" ("project_id")
    WHERE "project_role" = 'main';
CREATE INDEX "forms_by_project" ON "forms" ("project_id");

-- Who a form belongs to, beside its project's editors: who may see its
-- submissions.
CREATE TABLE "form_owners" (
    "form_id" TEXT NOT NULL REFERENCES "forms" ("form_id") ON DELETE CASCADE,
    "user_id" TEXT NOT NULL REFERENCES "users" ("user_id") ON DELETE CASCADE,
    PRIMARY KEY ("form_id", "user_id")
);

CREATE INDEX "form_owners_by_user" ON "form_owners" ("user_id");

CREATE TABLE "form_submissions" (
    "submission_id" TEXT PRIMARY KEY NOT NULL,
    "form_id" TEXT NOT NULL REFERENCES "forms" ("form_id") ON DELETE CASCADE,
    "orrery_id" INTEGER UNIQUE,
    "submitter" TEXT REFERENCES "users" ("user_id") ON DELETE CASCADE,
    "when" TEXT NOT NULL,
    -- JSON object: each answer, under its field's key. An answer under a
    -- key no field of the form asks for any longer is kept, not dropped.
    "answers" TEXT NOT NULL DEFAULT '{}',
    -- Where the request came from, and where that address was looked up to
    -- be. Kept apart from `location`, which is only ever the place the
    -- submitter gave.
    "ip_address" TEXT,
    -- What the people running the form judged it to be (common's
    -- `Verdict`), if they have.
    "evaluation" TEXT CHECK ("evaluation" IN ('positive', 'negative', 'invalid')),
    "comment" TEXT NOT NULL DEFAULT ''
);

SELECT AddGeometryColumn('form_submissions', 'location', 4326, 'POINT', 'XY');
SELECT AddGeometryColumn('form_submissions', 'ip_location', 4326, 'POINT', 'XY');
SELECT CreateSpatialIndex('form_submissions', 'location');

CREATE INDEX "form_submissions_by_form" ON "form_submissions" ("form_id", "when");
CREATE INDEX "form_submissions_by_submitter" ON "form_submissions" ("submitter");
