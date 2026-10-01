-- The E2E database: a table with a single-column primary key, so its result is editable.
CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL, plan TEXT NOT NULL);
INSERT INTO people (id, name, plan) VALUES (1, 'Ann', 'Free'), (2, 'Bo', 'Scale');
-- A table without a primary key: its results must stay read-only and say why.
CREATE TABLE notes (body TEXT NOT NULL);
INSERT INTO notes (body) VALUES ('first'), ('second');
