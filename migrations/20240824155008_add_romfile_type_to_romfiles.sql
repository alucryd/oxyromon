ALTER TABLE romfiles
ADD COLUMN romfile_type INTEGER NOT NULL DEFAULT 0;

UPDATE romfiles
SET romfile_type = 1
WHERE path LIKE '%.m3u';