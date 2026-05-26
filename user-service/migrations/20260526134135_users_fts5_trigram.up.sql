CREATE VIRTUAL TABLE users_fts USING fts5(
    handle,
    first_name,
    last_name,
    primary_email,
    content='users',
    content_rowid='rowid',
    tokenize='trigram'
);

INSERT INTO users_fts(rowid, handle, first_name, last_name, primary_email)
SELECT rowid, handle, first_name, last_name, primary_email FROM users;

CREATE TRIGGER users_fts_after_insert AFTER INSERT ON users BEGIN
    INSERT INTO users_fts(rowid, handle, first_name, last_name, primary_email)
    VALUES (new.rowid, new.handle, new.first_name, new.last_name, new.primary_email);
END;

CREATE TRIGGER users_fts_after_delete AFTER DELETE ON users BEGIN
    INSERT INTO users_fts(users_fts, rowid, handle, first_name, last_name, primary_email)
    VALUES ('delete', old.rowid, old.handle, old.first_name, old.last_name, old.primary_email);
END;

CREATE TRIGGER users_fts_after_update AFTER UPDATE OF handle, first_name, last_name, primary_email ON users BEGIN
    INSERT INTO users_fts(users_fts, rowid, handle, first_name, last_name, primary_email)
    VALUES ('delete', old.rowid, old.handle, old.first_name, old.last_name, old.primary_email);
    INSERT INTO users_fts(rowid, handle, first_name, last_name, primary_email)
    VALUES (new.rowid, new.handle, new.first_name, new.last_name, new.primary_email);
END;
