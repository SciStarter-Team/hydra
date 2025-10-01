-- Add composite index for autocomplete queries that filter by kind and search by name
create index pagecontent_by_kind_and_name on pagecontent("kind", "name");