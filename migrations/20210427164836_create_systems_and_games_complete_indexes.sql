CREATE INDEX IF NOT EXISTS systems_complete ON systems (complete);

CREATE INDEX IF NOT EXISTS games_complete ON games (complete);

CREATE INDEX IF NOT EXISTS games_sorting ON games (sorting);

CREATE INDEX IF NOT EXISTS games_complete_sorting ON games (complete, sorting);
