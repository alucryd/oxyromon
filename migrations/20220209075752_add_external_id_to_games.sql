ALTER TABLE games
ADD COLUMN external_id VARCHAR;

CREATE UNIQUE INDEX IF NOT EXISTS games_external_id_system_id ON games (external_id, system_id)
WHERE external_id IS NOT NULL;