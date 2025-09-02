-- Restore foreign key constraint to user_badges table
-- This recreates the table with the original foreign key constraint

-- Create new table with foreign key constraint
create table "user_badges_new" (
    "user_id" text not null references "users" ("user_id") on delete cascade,
    "badgeclass" text not null,
    "when" text not null, -- ISO 8601 timestamp when badge was earned
    "image" text -- nullable, empty string from sync treated as null
);

-- Copy data from old table to new table
insert into "user_badges_new" select * from "user_badges";

-- Drop old table
drop table "user_badges";

-- Rename new table to original name
alter table "user_badges_new" rename to "user_badges";

-- Recreate indexes
create index "user_badges_by_user_id" on "user_badges" ("user_id");
create index "user_badges_by_badgeclass" on "user_badges" ("badgeclass");
create index "user_badges_by_when" on "user_badges" ("when");
create unique index "user_badges_unique" on "user_badges" ("user_id", "badgeclass");