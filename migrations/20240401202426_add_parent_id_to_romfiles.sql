ALTER TABLE romfiles
ADD COLUMN parent_id INTEGER REFERENCES romfiles(id) ON DELETE SET NULL;
