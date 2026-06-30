CREATE TABLE IF NOT EXISTS history (
    id          INTEGER PRIMARY KEY,
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER,
    failed      INTEGER NOT NULL DEFAULT 0
);