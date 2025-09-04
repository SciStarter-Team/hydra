-- Note: Cannot disable foreign keys inside transaction, so we'll recreate referencing tables

-- Disable spatial indexes first to avoid conflicts during table operations
SELECT DisableSpatialIndex('users', 'point');
SELECT DisableSpatialIndex('users', 'regions');

-- Drop the actual spatial index tables (DisableSpatialIndex only disables triggers)
DROP TABLE IF EXISTS idx_users_point;
DROP TABLE IF EXISTS idx_users_point_node;
DROP TABLE IF EXISTS idx_users_point_parent;
DROP TABLE IF EXISTS idx_users_point_rowid;
DROP TABLE IF EXISTS idx_users_regions;
DROP TABLE IF EXISTS idx_users_regions_node;
DROP TABLE IF EXISTS idx_users_regions_parent; 
DROP TABLE IF EXISTS idx_users_regions_rowid;

-- Discard geometry columns to clean up all spatial metadata, triggers, and constraints
SELECT DiscardGeometryColumn('users', 'point');
SELECT DiscardGeometryColumn('users', 'regions');

-- Remove the unique index on username first
DROP INDEX IF EXISTS "users_by_username";

-- Backup referencing tables to avoid foreign key constraint issues
CREATE TABLE "emails_backup" AS SELECT * FROM "emails";
CREATE TABLE "user_tags_backup" AS SELECT * FROM "user_tags";

-- Drop the referencing tables temporarily
DROP TABLE "emails";
DROP TABLE "user_tags";

-- Create new table structure without username column directly
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
       "ambassador" text, -- datetime
       "is_page_manager" integer NOT NULL DEFAULT 0,
       "point" BLOB,
       "regions" BLOB
);

-- Copy data from old table to new table (excluding the username column but preserving geometry)
INSERT INTO users_new SELECT 
    user_id, legacy_v2_id, legacy_v2_userdata_id, legacy_v3_id, legacy_v2_profile_id, 
    legacy_v3_alias_id, legacy_v3_profile_id, date_joined, updated, password, 
    handle, primary_email, verified, is_superuser, is_staff, is_test, is_active, 
    public, accept_emails, newsletter, date_unsubscribed, accept_messages, 
    track_contributions, first_name, last_name, image, birthday, gender, bio, 
    facebook_url, twitter_profile, inat_username, inat_last_update, zipcode, 
    country, mailing_address, scientist, educator, origin, referer, utm_source, 
    utm_medium, utm_campaign, utm_term, utm_content, misc, restricted_navigation, 
    protected, extra_skills, extra_tools, api_key, site_dressing, country_code, 
    needs_inat_username, opaque_id, zooniverse_username, degree, ethnicity, 
    income, ai_opt_out, ai_opt_out_at, umbrella_project_id, automatic_affiliate, 
    circuit_id, home_page, constant_contact_id, ambassador, is_page_manager, point, regions
FROM users;

-- Drop the original table using standard SQL (spatial metadata already cleaned up)
DROP TABLE "users";

-- Rename the new table using standard SQL
ALTER TABLE "users_new" RENAME TO "users";

-- Register the geometry columns with SpatiaLite (this recreates the spatial metadata and triggers)
SELECT RecoverGeometryColumn('users', 'point', 4326, 'POINT', 'XY');
SELECT RecoverGeometryColumn('users', 'regions', 4326, 'MULTIPOLYGON', 'XY');

-- Recreate spatial indexes
SELECT CreateSpatialIndex('users', 'point');
SELECT CreateSpatialIndex('users', 'regions');

-- Create the user_id index (but not the username index)
CREATE UNIQUE INDEX "users_by_user_id" ON "users" ("user_id");

-- Recreate the referencing tables with their original structure and data
CREATE TABLE "emails" (
       "user_id" text not null references "users" ("user_id") on delete cascade,
       "email" text not null,
       "verified" boolean not null,
       "hash" text not null,
       "hash_lower" text not null,
       "hash_capfirst" text not null
);

CREATE TABLE "user_tags" (
       "user_id" text not null references "users" ("user_id") on delete cascade,
       "tag" text not null
);

-- Restore the data from backup tables
INSERT INTO "emails" SELECT * FROM "emails_backup";
INSERT INTO "user_tags" SELECT * FROM "user_tags_backup";

-- Drop the backup tables
DROP TABLE "emails_backup";
DROP TABLE "user_tags_backup";

-- Recreate indexes for the restored tables
CREATE UNIQUE INDEX "emails_by_email" ON "emails" ("email");
CREATE UNIQUE INDEX "emails_by_hash" ON "emails" ("hash");
CREATE UNIQUE INDEX "emails_by_hash_lower" ON "emails" ("hash_lower");
CREATE UNIQUE INDEX "emails_by_hash_capfirst" ON "emails" ("hash_capfirst");

CREATE UNIQUE INDEX "user_tags_by_user_id_and_tag" ON "user_tags" ("user_id", "tag");
CREATE INDEX "user_tags_by_user_id" ON "user_tags" ("user_id");
CREATE INDEX "user_tags_by_tag" ON "user_tags" ("tag");
