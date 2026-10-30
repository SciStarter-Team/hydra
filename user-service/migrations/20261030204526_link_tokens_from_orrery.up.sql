-- Link Tokens made on orrery are copied here by the link-token sync, keeping
-- their token strings, so that an affiliate holding one goes on reporting
-- through it after cutover. orrery's row id marks a copied token; it is null
-- on a token made here.
alter table "link_tokens" add column "orrery_id" integer;

create unique index "link_tokens_by_orrery_id" on "link_tokens" ("orrery_id");

-- A token never expired: the endpoint did not consult this, and orrery's
-- tokens have no expiry to carry.
alter table "link_tokens" drop column "expires";
