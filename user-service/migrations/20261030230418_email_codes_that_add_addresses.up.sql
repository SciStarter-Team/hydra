-- A code mailed to an address its account does not have yet, at its owner's
-- asking: redeeming it adds the address to the account, confirmed. The
-- address is kept out of "emails" until then, since an address there is one
-- nobody else can sign up with, and it is not the account's to hold until
-- its owner shows they read its mail.
alter table "email_verification_codes"
    add column "adds_address" boolean not null default 0;

create index "email_verification_codes_by_user" on "email_verification_codes" ("user_id", "adds_address");
