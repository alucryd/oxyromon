CREATE TABLE IF NOT EXISTS settings (
    id INTEGER NOT NULL PRIMARY KEY,
    key VARCHAR NOT NULL,
    value VARCHAR
);

INSERT OR REPLACE INTO settings ("key", value)
VALUES('DISCARD_BETA', 'false');

INSERT OR REPLACE INTO settings ("key", value)
VALUES('DISCARD_DEBUG', 'false');

INSERT OR REPLACE INTO settings ("key", value)
VALUES('DISCARD_DEMO', 'false');

INSERT OR REPLACE INTO settings ("key", value)
VALUES('DISCARD_PROGRAM', 'false');

INSERT OR REPLACE INTO settings ("key", value)
VALUES('DISCARD_PROTO', 'false');

INSERT OR REPLACE INTO settings ("key", value)
VALUES('DISCARD_SAMPLE', 'false');

INSERT OR REPLACE INTO settings ("key", value)
VALUES('DISCARD_SEGA_CHANNEL', 'false');

INSERT OR REPLACE INTO settings ("key", value)
VALUES('DISCARD_VIRTUAL_CONSOLE', 'false');
