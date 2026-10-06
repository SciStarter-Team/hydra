-- Codes mailed to an address, so that whoever reads its mail can confirm the
-- address is theirs. Redeeming one marks the address verified; a code is good
-- for 7 days.
--
-- Each row is also the record of a confirmation email having been sent, which
-- is what limits how often an account may ask for another.
create table "email_verification_codes" (
    "code" text primary key not null,
    "user_id" text not null references "users" ("user_id") on delete cascade,
    "email" text not null,
    "issued" text not null
);

create index "email_verification_codes_by_email" on "email_verification_codes" ("email", "issued");
