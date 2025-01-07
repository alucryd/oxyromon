ALTER TABLE systems
ADD COLUMN completion SMALLINT NOT NULL DEFAULT 0;

UPDATE systems
SET completion = 2
WHERE complete = true;

CREATE INDEX IF NOT EXISTS systems_completion ON systems (completion);

DROP INDEX systems_complete;

ALTER TABLE systems
DROP COLUMN complete;
