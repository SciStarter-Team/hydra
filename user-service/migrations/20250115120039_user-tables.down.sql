drop index if exists "user_tags_by_user_and_tag";
drop table if exists "user_tags";
drop index if exists "tags_by_tag";
drop table if exists "tags";
drop index if exists "emails_by_hash_capfirst";
drop index if exists "emails_by_hash_lower";
drop index if exists "emails_by_hash";
drop index if exists "emails_by_email";
drop table if exists "emails";
drop index if exists "users_by_username";
drop index if exists "users_by_user_id";
select DropTable(NULL, 'users', 1); -- also cleans up Spatialite data associated with the table
