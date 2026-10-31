-- Signup tags whose cohort is credited with a training outright: badge names
-- a `SignupTagBadge` by its stored spelling. Staff keep these in orrery's
-- admin and the signup-tag-grant sync copies them here; orrery's row id marks
-- a copy, and is null on a grant made in hydra's admin.
--
-- No uniqueness on (tag, badge): a copy and a grant made here may name the
-- same pair while both systems are in use, and either one credits the same.
-- A grant made here is refused if the pair is already granted.
create table "signup_tag_grants" (
    "id" integer primary key not null,
    "tag" text not null,
    "badge" text not null,
    "created" text not null,
    "orrery_id" integer
);

create unique index "signup_tag_grants_by_orrery_id" on "signup_tag_grants" ("orrery_id");

create index "signup_tag_grants_by_tag" on "signup_tag_grants" ("tag");
