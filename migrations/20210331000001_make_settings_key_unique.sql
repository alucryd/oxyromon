CREATE TABLE settings_old AS SELECT * FROM settings;

DROP TABLE settings;

CREATE TABLE IF NOT EXISTS settings (
    id INTEGER NOT NULL PRIMARY KEY,
    key VARCHAR NOT NULL UNIQUE,
    value VARCHAR
);

INSERT INTO settings (key, value)
SELECT key, value FROM settings_old
GROUP BY key
ORDER BY key;

DROP TABLE settings_old;
