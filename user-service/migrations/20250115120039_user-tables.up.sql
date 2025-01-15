create table "users" (
       "user_id" text not null,
       "legacy_v2_id" integer,
       "legacy_v2_userdata_id" integer,
       "legacy_v3_id" integer,
       "date_joined" text not null,
       "last_login"text not null,
       "updated" text not null,
       "username" text not null,
       "password" text not null,
       "handle" text not null,
       "primary_email" text not null,
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
) strict;

select AddGeometryColumn('users', 'point', 4326, 'POINT', 'XY');
select AddGeometryColumn('users', 'regions', 4326, 'MULTIPOLYGON', 'XY');

select CreateSpatialIndex('users', 'point');
select CreateSpatialIndex('users', 'regions');

create unique index "users_by_user_id" on "users" ("user_id");
create unique index "users_by_username" on "users" ("username");

create table "emails" (
       "user_id" text not null references "users" ("user_id") on delete cascade,
       "email" text not null,
       "hash" text not null,
       "hash_lower" text not null,
       "hash_capfirst" text not null,
) strict;

create unique index "emails_by_email" on "emails" ("email");
create unique index "emails_by_hash" on "emails" ("hash");
create unique index "emails_by_hash_lower" on "emails" ("hash_lower");
create unique index "emails_by_hash_capfirst" on "emails" ("hash_capfirst");

create table "tags" (
       "tag" text not null,
) strict;

create unique index "tags_by_tag" on "tags" ("tag");

create table "user_tags" (
       "user_id" text not null references "users" ("user_id") on delete cascade,
       "tag" text not null references "tags" ("tag") on delete cascade
) strict;

create unique index "user_tags_by_user_and_tag" on "user_tags" ("user_id", "tag");
