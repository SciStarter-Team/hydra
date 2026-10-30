alter table "link_tokens" add column "expires" text;

drop index "link_tokens_by_orrery_id";

alter table "link_tokens" drop column "orrery_id";
