create table "sessions" (
       "session_id" text primary key not null,
       "expires" text,
       "data" blob not null
) strict;

create index "sessions_by_expires" on "sessions" ("expires");
