create index users_by_is_staff on "users" ("date_joined" desc) where "is_staff" = 1;
create index users_by_is_page_manager on "users" ("date_joined" desc) where "is_page_manager" = 1;
create index users_by_verified on "users" ("date_joined" desc) where "verified" = 1;
create index uses_by_no_role on "users" ("date_joined" desc) where "is_staff" = 0 and "is_page_manager" = 0;
