create table "user_badges" (
    "user_id" text not null references "users" ("user_id") on delete cascade,
    "badgeclass" text not null,
    "when" text not null, -- ISO 8601 timestamp when badge was earned
    "image" text -- nullable, empty string from sync treated as null
);

create index "user_badges_by_user_id" on "user_badges" ("user_id");
create index "user_badges_by_badgeclass" on "user_badges" ("badgeclass");
create index "user_badges_by_when" on "user_badges" ("when");
create unique index "user_badges_unique" on "user_badges" ("user_id", "badgeclass");