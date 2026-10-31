-- Where the link handing out a grant's tag sends people once they have
-- signed up or signed in: a path on this site, or empty for nowhere in
-- particular. Only the link carries it; a grant credits its tag however the
-- tag arrives.
alter table "signup_tag_grants" add column "next_path" text not null default '';
