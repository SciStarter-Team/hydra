select InitSpatialMetaDataFull() where not exists(select 1 from sqlite_master where type = 'table' and name = 'spatial_ref_sys');

create table "entities" (
       "slug" text not null,
       "kind" text not null,
       "target" text not null
);

create unique index "entities_by_slug" on "entities" ("slug");
create index "entities_by_kind" on "entities" ("kind");
create unique index "entities_by_target" on "entities" ("target");

create table "redirects" (
       "redirect_id" text not null,
       "target" text not null
);

create unique index "redirects_by_redirect_id" on "redirects" ("redirect_id");

create table "html_stylesheet_urls" (
       "html_id" text not null,
       "url" text not null
);

create index "html_stylesheet_urls_by_html_id" on "html_stylesheet_urls" ("html_id");

create table "html_script_urls" (
       "html_id" text not null,
       "url" text not null
);

create index "html_script_urls_by_html_id" on "html_script_urls" ("html_id");

create table "htmls" (
       "html_id" text not null,
       "content" text not null
);

create unique index "htmls_by_html_id" on "htmls" ("html_id");

create table "project_tags" (
       "project_id" text not null,
       "tag" text not null
);

create index "project_tags_by_project_id" on "project_tags" ("project_id");
create index "project_tags_by_tag" on "project_tags" (upper("tag"));
create unique index "project_tags_by_project_and_tag" on "project_tags" ("project_id", upper("tag"));

create table "project_page_editors" (
       "project_id" text not null,
       "user_id" text not null
);

create index "project_page_editors_by_project_id" on "project_page_editors" ("project_id");
create index "project_page_editors_by_user_id" on "project_page_editors" ("user_id");

create table "project_participant_files" (
       "project_id" text not null,
       "url" text not null
);

create index "project_participant_files_by_project_id" on "project_participant_files" ("project_id");

create table "project_contacts" (
       "contact_id" text not null,
       "project_id" text not null,
       "role" text not null,
       "name" text,
       "affiliation" text,
       "email" text not null,
       "phone" text,
       "address" text
);

create unique index "project_contacts_by_contact_id" on "project_contacts" ("contact_id");
create index "project_contacts_by_project_id" on "project_contacts" ("project_id");
create index "project_contacts_by_role" on "project_contacts" ("role");
create index "project_contacts_by_email" on "project_contacts" (upper("email"));

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

create table "project_data_aggregations" (
       "project_id" text not null,
       "data_aggregation" text not null
);

create index "project_data_aggregations_by_project_id" on "project_data_aggregations" ("project_id");

create table "project_data_consents" (
       "project_id" text not null,
       "data_consent" text not null
);

create index "project_data_consents_by_project_id" on "project_data_consents" ("project_id");

create table "project_data_access" (
       "project_id" text not null,
       "data_access" text not null
);

create index "project_data_access_by_project_id" on "project_data_access" ("project_id");

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
       "authoritative" boolean not null,
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
       "page_creator_id" text not null default '', -- NB was "owner"
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

       -- section Relations
       "part_of" text,

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

select AddGeometryColumn('projects', 'point', 4326, 'POINT', 'XY');
select AddGeometryColumn('projects', 'regions', 4326, 'MULTIPOLYGON', 'XY');

select CreateSpatialIndex('projects', 'point');
select CreateSpatialIndex('projects', 'regions');

create unique index "projects_by_project_id" on "projects" ("project_id");
create index "projects_by_legacy_v1_project_id" on "projects" ("legacy_v1_project_id") where "legacy_v1_project_id" not null;
create index "projects_by_legacy_v2_project_id" on "projects" ("legacy_v2_project_id") where "legacy_v2_project_id" not null;

create table "event_tags" (
       "event_id" text not null,
       "tag" text not null
);

create index "event_tags_by_event_id" on "event_tags" ("event_id");
create index "event_tags_by_tag" on "event_tags" (upper("tag"));
create unique index "event_tags_by_event_and_tag" on "event_tags" ("event_id", upper("tag"));

create table "event_page_editors" (
       "event_id" text not null,
       "user_id" text not null
);

create index "event_page_editors_by_event_id" on "event_page_editors" ("event_id");
create index "event_page_editors_by_user_id" on "event_page_editors" ("user_id");

create table "event_contacts" (
       "contact_id" text not null,
       "event_id" text not null,
       "role" text not null,
       "name" text,
       "affiliation" text,
       "email" text not null,
       "phone" text,
       "address" text
);

create unique index "event_contacts_by_contact_id" on "event_contacts" ("contact_id");
create index "event_contacts_by_event_id" on "event_contacts" ("event_id");
create index "event_contacts_by_role" on "event_contacts" ("role");
create index "event_contacts_by_email" on "event_contacts" (upper("email"));

create table "event_instances" (
       "event_id" text not null,
       "begin" text not null,
       "end" text not null
);

create index "event_instances_by_event_id" on "event_instances" ("event_id");
create index "event_instances_by_begin" on "event_instances" ("begin");
create index "event_instances_by_end" on "event_instances" ("end");

create table "events" (
       "event_id" text not null, -- Ulid
       "created" text not null, -- TimeString
       "publish" boolean not null, -- bool
       "flagged" boolean not null, -- bool
       "admin_notes" text not null, -- bool
       "welcomed" boolean not null, -- bool
       "page_creator_id" text not null, -- Ulid
       "name" text not null,
       "description" text not null, -- String,
       "origin" text not null, -- String,
       "presenter" text not null, -- String,
       "url" text not null, -- String,
       "image" text not null, -- String,
       "image_credit" text not null, -- String,
       "how_to_join" text not null, -- String,
       "part_of" text, -- Option<Ulid>,
       "location_type" text not null, -- String,
       "entirely_online" boolean not null, -- bool,
       "location_name" text not null, -- String,
       "location_description" text not null, -- String,
       "address" text not null, -- String,
       "city" text not null, -- String,
       "zip" text not null, -- String,
       "state" text not null, -- String,
       "country" text not null, -- String,
       "event_type" text not null, -- EventType,
       "ticketing_begins" text, -- Option<TimeString>,
       "outcomes_label" text, -- Option<String>,
       "outcomes_link" text, -- Option<String>,
       "circuit_id" text -- Option<String>
);

select AddGeometryColumn('events', 'point', 4326, 'POINT', 'XY');
select AddGeometryColumn('events', 'regions', 4326, 'MULTIPOLYGON', 'XY');

select CreateSpatialIndex('events', 'point');
select CreateSpatialIndex('events', 'regions');

create unique index "events_by_event_id" on "events" ("event_id");
