-- This migration adds back the username field (for rollback purposes)
-- This is a complex rollback since we're recreating the table structure

-- Disable foreign key constraints during migration
PRAGMA foreign_keys = OFF;

-- Create new table with username field restored
CREATE TABLE "users_new" (
       "user_id" text not null,
       "legacy_v2_id" integer,
       "legacy_v2_userdata_id" integer,
       "legacy_v3_id" integer,
       "legacy_v2_profile_id" integer,
       "legacy_v3_alias_id" integer,
       "legacy_v3_profile_id" integer,
       "date_joined" text not null,
       "updated" text not null default current_timestamp,
       "username" text not null,
       "password" text not null,
       "handle" text not null,
       "primary_email" text not null,
       "verified" boolean not null,
       "is_superuser" integer not null default 0,
       "is_staff" integer not null default 0,
       "is_test" integer not null default 0,
       "is_active" integer not null default 1,
       "public" integer not null default 1,
       "accept_emails" integer not null default 1,
       "newsletter" integer not null default 1,
       "date_unsubscribed" text, -- datetime
       "accept_messages" integer not null default 1,
       "track_contributions" integer not null default 1,
       "first_name" text not null default '',
       "last_name" text not null default '',
       "image" text,
       "birthday" text, -- date
       "gender" text,
       "bio" text,
       "facebook_url" text,
       "twitter_profile" text,
       "inat_username" text,
       "inat_last_update" text,
       "zipcode" text,
       "country" text,
       "mailing_address" text,
       "scientist" integer not null default 0,
       "educator" integer not null default 0,
       "origin" text not null default '',
       "referer" text not null default '',
       "utm_source" text not null default '',
       "utm_medium" text not null default '',
       "utm_campaign" text not null default '',
       "utm_term" text not null default '',
       "utm_content" text not null default '',
       "misc" text not null default '',
       "restricted_navigation" integer not null default 0,
       "protected" integer not null default 0,
       "extra_skills" text not null default '',
       "extra_tools" text not null default '',
       "api_key" text,
       "site_dressing" integer not null default 0,
       "country_code" text not null default 'US',
       "needs_inat_username" integer not null default 0,
       "opaque_id" text,
       "zooniverse_username" text not null default '',
       "degree" text,
       "ethnicity" text,
       "income" text,
       "ai_opt_out" integer not null default 0,
       "ai_opt_out_at" text,  -- datetime
       "umbrella_project_id" text,
       "automatic_affiliate" integer not null default 0,
       "circuit_id" text,
       "home_page" text,
       "constant_contact_id" text,
       "ambassador" text -- datetime
);

-- Copy data back (adding username field with primary_email as default)
INSERT INTO users_new SELECT 
    user_id, legacy_v2_id, legacy_v2_userdata_id, legacy_v3_id, legacy_v2_profile_id, 
    legacy_v3_alias_id, legacy_v3_profile_id, date_joined, updated, primary_email, password, 
    handle, primary_email, verified, is_superuser, is_staff, is_test, is_active, 
    public, accept_emails, newsletter, date_unsubscribed, accept_messages, 
    track_contributions, first_name, last_name, image, birthday, gender, bio, 
    facebook_url, twitter_profile, inat_username, inat_last_update, zipcode, 
    country, mailing_address, scientist, educator, origin, referer, utm_source, 
    utm_medium, utm_campaign, utm_term, utm_content, misc, restricted_navigation, 
    protected, extra_skills, extra_tools, api_key, site_dressing, country_code, 
    needs_inat_username, opaque_id, zooniverse_username, degree, ethnicity, 
    income, ai_opt_out, ai_opt_out_at, umbrella_project_id, automatic_affiliate, 
    circuit_id, home_page, constant_contact_id, ambassador
FROM users;

-- Drop old table and rename new table
DROP TABLE users;
ALTER TABLE users_new RENAME TO users;

-- Recreate spatial metadata and geometry columns
SELECT AddGeometryColumn('users', 'point', 4326, 'POINT', 'XY');
SELECT AddGeometryColumn('users', 'regions', 4326, 'MULTIPOLYGON', 'XY');

-- Recreate spatial indexes
SELECT CreateSpatialIndex('users', 'point');
SELECT CreateSpatialIndex('users', 'regions');

-- Recreate both indexes
CREATE UNIQUE INDEX "users_by_user_id" ON "users" ("user_id");
CREATE UNIQUE INDEX "users_by_username" ON "users" ("username");

-- Re-enable foreign key constraints
PRAGMA foreign_keys = ON;
