create table "entity" (
       "slug" text not null,
       "kind" text not null,
       "target" text not null,
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

create table "project_contact" (
       "project_id" text not null,
       "contact_id" text not null
);

create index "project_contact_by_project_id" on "project_contact" ("project_id");
create index "project_contact_by_contact_id" on "project_contact" ("contact_id");
