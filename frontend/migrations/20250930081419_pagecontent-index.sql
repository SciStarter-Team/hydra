create unique index pagecontent_by_id on pagecontent ("pagecontent_id");
create index pagecontent_by_id_and_kind on pagecontent ("pagecontent_id", "kind");
create index pagecontent_by_name on pagecontent("name");
