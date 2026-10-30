drop index "email_verification_codes_by_user";

alter table "email_verification_codes" drop column "adds_address";
