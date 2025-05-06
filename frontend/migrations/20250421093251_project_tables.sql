create table "entity" (
       "slug" text not null,
       "kind" text not null,
       "target" text not null
);

create unique index "entity_by_slug" on "entity" ("slug");
create index "entity_by_kind" on "entity" ("kind");
create unique index "entity_by_target" on "entity" ("target");

create table "project_tag" (
       "project_id" text not null,
       "tag" text not null
);

create index "project_tag_by_project_id" on "project_tag" ("project_id");
create index "project_tag_by_tag" on "project_tag" (upper("tag"));
create unique index "project_tag_by_project_and_tag" on "project_tag" ("project_id", upper("tag"));

create table "project_manager" (
       "project_id" text not null,
       "user_id" text not null
);

create index "project_manager_by_project_id" on "project_manager" ("project_id");
create index "project_manager_by_user_id" on "project_manager" ("user_id");

create table "project_participant_files" (
       "project_id" text not null,
       "url" text not null
);

create index "project_participant_files_by_project_id" on "project_manager" ("project_id");

create table "contact" (
       "contact_id" text not null,
       "role" text not null,
       "name" text,
       "affiliation" text,
       "email" text not null,
       "phone" text,
       "address" text
);

create unique index "contact_by_contact_id" on "contact" ("contact_id");
create index "contact_by_role" on "contact" ("role");
create index "contact_by_email" on "contact" (upper("email"));

create table "project_contacts" (
       "project_id" text not null,
       "contact_id" text not null
);

create index "project_contacts_by_project_id" on "project_contacts" ("project_id");
create index "project_contacts_by_contact_id" on "project_contacts" ("contact_id");

create table "project_topics" (
       "project_id" text not null,
       "topic" text not null
);

create index "project_topics_by_project_id" on "project_topics" ("project_id");
create index "project_topics_by_topic" on "project_topics" ("topic");

create table "project_fields_of_science" (
       "project_id" text not null,
       "field_of_science" text not null
);

create index "project_fields_of_science_by_project_id" on "project_fields_of_science" ("project_id");
create index "project_fields_of_science_by_field_of_science" on "project_fields_of_science" ("field_of_science");

create table "project_practices" (
       "project_id" text not null,
       "practice" text not null
);

create index "project_practices_by_project_id" on "project_practices" ("project_id");
create index "project_practices_by_practice" on "project_practices" ("practice");

create table "project_audiences" (
       "project_id" text not null,
       "audience" text not null
);

create index "project_audiences_by_project_id" on "project_audiences" ("project_id");
create index "project_audiences_by_audience" on "project_audiences" ("audience");

create table "project_associated_projects" (
       "project_id" text not null,
       "associated_project_id" text not null
);

create index "project_associated_projects_by_project_id" on "project_associated_projects" ("project_id");
create index "project_associated_projects_by_associated_project_id" on "project_associated_projects" ("associated_project_id");

create table "project_geographic_scopes" (
       "project_id" text not null,
       "geographic_scope" text not null
);

create index "project_geographic_scopes_by_project_id" on "project_geographic_scopes" ("project_id");
create index "project_geographic_scopes_by_geographic_scope" on "project_geographic_scopes" ("geographic_scope");

create table "project_participation_tasks" (
       "project_id" text not null,
       "participation_task" text not null
);

create index "project_participation_tasks_by_project_id" on "project_participation_tasks" ("project_id");
create index "project_participation_tasks_by_participation_task" on "project_participation_tasks" ("participation_task");

create table "project_activities" (
       "project_id" text not null,
       "activity" text not null
);

create index "project_activities_by_project_id" on "project_activities" ("project_id");
create index "project_activities_by_activity" on "project_activities" ("activity");

create table "project_aggregations" (
       "project_id" text not null,
       "aggregation" text not null
);

create index "project_aggregations_by_project_id" on "project_aggregations" ("project_id");

create table "project_consents" (
       "project_id" text not null,
       "consent" text not null
);

create index "project_consents_by_project_id" on "project_consents" ("project_id");

create table "project_access" (
       "project_id" text not null,
       "access" text not null
);

create index "project_access_by_project_id" on "project_access" ("project_id");

create table "project_intended_outcomes" (
       "project_id" text not null,
       "intended_outcome" text not null
);

create index "project_intended_outcomes_by_project_id" on "project_intended_outcomes" ("project_id");

create table "project_sponsors" (
       "project_id" text not null,
       "sponsor" text not null
);

create index "project_sponsors_by_project_id" on "project_sponsors" ("project_id");

create table "project_stakeholders" (
       "project_id" text not null,
       "stakeholder" text not null
);

create index "project_stakeholders_by_project_id" on "project_stakeholders" ("project_id");

create table "projects" (
       "project_id" text not null,
       "legacy_v1_project_id" integer,
       "legacy_v2_project_id" integer,
       "foreign_id" text,

       -- section Admin
       "publish" boolean,
       "duplicate" boolean not null default false,
       "flagged" boolean not null default false,
       "is_test" boolean not null default false,
       "status" text not null,
       "promotion_time" text not null default '',
       "promotion_target" text,
       "admin_notes" text not null default '',
       "created" text not null default '',
       "wild_hope" boolean not null default false,
       "exchange" text,
       "origin" text not null default '',
       "suborigin" text not null default '',
       "guid" text not null default '',
       "opengraph_description" text not null default '',
       "is_affiliate" boolean not null default false,
       "is_inaturalist" boolean not null default false,
       "inat_project_id" integer,
       "welcomed" boolean not null default false,
       "page_creator" text not null default '', -- NB was "owner"
       "extra" text not null default '',

       -- section Project
       "draft" boolean not null default false,
       "name" text not null,
       "participation_degree" text not null default '',
       "participant_instructions" text not null default '',
       "participant_instructions_email" text not null default '',
       "us_federally_funded" boolean not null default false,
       "presenter" text not null default '',
       "goal" text not null default '',
       "task" text not null default '',
       "description" text not null default '',
       "image" text not null default '',
       "image_credit" text not null default '',
       "connect_opaque_url" text not null default '',
       "connect_opaque_url1" text not null default '',
       "connect_opaque_url2" text not null default '',

       -- section Web
       "url" text not null default '',
       "video_url" text not null default '',
       "blog_url" text not null default '',
       "twitter_name" text not null default '',
       "facebook_page" text not null default '',
       "instagram_page" text not null default '',

       -- section Categorization
       "practices_how" text not null default '',
       "search_terms" text not null default '',
       "difficulty" text not null default '',
       "diy" boolean not null default false,

       -- section Joining
       "signup_url" text not null default '',
       "prefix_how_to_logged_in" text not null default '',
       "prefix_how_to_logged_out" text not null default '',
       "how_to_join" text not null default '',
       "howto_video" text not null default '',
       "outcomes_label" text not null default '',
       "outcomes_link" text not null default '',
       "time_required" text not null default '',
       "training_materials" text not null default '',
       "classroom_materials" text not null default '',
       "consent_statement" text not null default '',
       "consent_statement_date" text,
       "participation_tools" boolean not null default false,
       "participation_prompt" text not null default '',
       "participation_confirmation_message" text not null default '',
       "participation_notification_email" text not null default '',
       "participation_usage" text not null default '',

       -- section Participants Need
       "total_expense" real,
       "materials" text not null default '',
       "gear" text not null default '',
       "mobile_app_url_android" text not null default '',
       "mobile_app_url_ios" text not null default '',
       "mobile_app_url" text not null default '',

       -- section Where
       "location_type" text not null default '',
       "entirely_online" boolean not null default false,
       "on_demand" boolean not null default false,
       "in_person" boolean not null default false,
       "indoors" boolean not null default false,
       "outdoors" boolean not null default false,
       "regions_description" text not null default '',
       "address" text not null default '',
       "city" text not null default '',
       "state" text not null default '',
       "zip" text not null default '',
       "country" text not null default '',
       "other" text not null default '',
       "location_name" text not null default '',
       "location_landuse" text not null default '',
       "location_description" text not null default '',
       "united_nations_region" text not null default '',

       -- section When
       "begin" text,
       "end" text,
       "project_timing" text not null default '',
       "participation_schedule" text not null default '',
       "time_per_participation" text not null default '',
       "year_started" integer,
       "year_completed" integer,
       "month_completed" integer,

       -- section Skills
       "special_skills" text not null default '',

       -- section Data
       "data_results" text not null default '',
       "data_visualization" boolean not null default false,
       "data_quality" boolean not null default false,
       "metadata_publication_url" text not null default '',
       "data_publication_url" text not null default '',
       "data_license" text not null default '',

       -- section Outcomes
       "outcome_other" text not null default '',

       -- section Sponsors
       "university_affiliation" text not null default '',
       "circuit_id" text not null default '',
       "sustainable_development_goals" text not null default ''
);

select InitSpatialMetaDataFull() where not exists(select 1 from sqlite_master where type = 'table' and name = 'spatial_ref_sys');

select AddGeometryColumn('projects', 'point', 4326, 'POINT', 'XY');
select AddGeometryColumn('projects', 'regions', 4326, 'MULTIPOLYGON', 'XY');

select CreateSpatialIndex('projects', 'point');
select CreateSpatialIndex('projects', 'regions');

create index "projects_by_project_id" on "projects" ("project_id");
create index "projects_by_legacy_v1_project_id" on "projects" ("legacy_v1_project_id") where "legacy_v1_project_id" not null;
create index "projects_by_legacy_v2_project_id" on "projects" ("legacy_v2_project_id") where "legacy_v2_project_id" not null;
