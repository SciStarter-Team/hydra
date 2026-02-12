CREATE TABLE news_items (
    news_id TEXT PRIMARY KEY NOT NULL,
    source_type TEXT NOT NULL,
    tag TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL DEFAULT '',
    author TEXT NOT NULL DEFAULT '',
    display_date TEXT NOT NULL DEFAULT '',
    url TEXT NOT NULL DEFAULT '',
    active INTEGER NOT NULL DEFAULT 1,
    sort_order INTEGER NOT NULL DEFAULT 0
);
