ALTER TABLE games
ADD COLUMN completion SMALLINT NOT NULL DEFAULT 0;

UPDATE games
SET completion = 2
WHERE complete = true;

CREATE INDEX IF NOT EXISTS games_completion ON games (completion);
CREATE INDEX IF NOT EXISTS games_completion_sorting ON games (completion, sorting);

DROP INDEX games_complete;
DROP INDEX games_complete_sorting;

ALTER TABLE games
DROP COLUMN complete;
