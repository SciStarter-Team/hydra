CREATE TABLE "ambassador_applications" (
    "user_id" text PRIMARY KEY NOT NULL REFERENCES "users" ("user_id") ON DELETE CASCADE,
    "bio" text NOT NULL DEFAULT '',
    "city" text NOT NULL DEFAULT '',
    "state" text NOT NULL DEFAULT '',
    "contact_email" text NOT NULL DEFAULT '',
    "youtube_video_url" text NOT NULL DEFAULT '',
    "app_status" integer NOT NULL DEFAULT 0,
    "submitted" text,
    "updated_at" text NOT NULL DEFAULT (current_timestamp)
);

CREATE INDEX "ambassador_applications_by_app_status" ON "ambassador_applications" ("app_status");
