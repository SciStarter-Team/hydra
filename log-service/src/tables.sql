create table "activity_log" (
       "when" text not null default current_timestamp,
       "action" text not null,
       "user_id" text,
       "target" text,
       "extra" text
);

create index "activity_log_by_when" on "activity_log" ("when");
create index "activity_log_by_action" on "activity_log" ("action");
create index "activity_log_by_user_id" on "activity_log" ("user_id") where "user_id" is not null;
create index "activity_log_by_target" on "activity_log" ("target") where "target" is not null;

