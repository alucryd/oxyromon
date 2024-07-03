ALTER TABLE systems
ADD COLUMN custom_name VARCHAR;

CREATE UNIQUE INDEX IF NOT EXISTS systems_custom_name ON systems (custom_name);