-- Signups deferred to email verification because Turnstile could not vouch
-- for the request. No users row exists until the emailed completion link is
-- clicked; rows expire after 7 days.
create table "pending_signups" (
    "code" text primary key not null,
    "email" text not null,
    "email_lower" text not null unique,
    "handle" text not null,
    "password" text not null,
    "newsletter" integer not null default 0,
    "zipcode" text not null default '',
    "protected" integer not null default 0,
    "point" text,
    "tags" text not null default '',
    "next" text,
    "ip_address" text not null default '',
    "created" text not null,
    "email_sent" text not null
);

create index "pending_signups_by_ip" on "pending_signups" ("ip_address", "email_sent");
