ALTER TABLE games
ADD COLUMN playlist_id INTEGER REFERENCES romfiles(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS games_playlist_id ON games (playlist_id);
