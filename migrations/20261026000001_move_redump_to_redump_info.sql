-- Redump moved from redump.org to redump.info and renamed some systems.
-- A renamed system keeps its directory: its old name becomes its custom name,
-- unless it already has one.

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Acorn - Archimedes & Risc PC'
WHERE name = 'Acorn - Archimedes' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Acorn - Archimedes & Risc PC');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Apple - Pippin'
WHERE name = 'Bandai - Pippin' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Apple - Pippin');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Fujitsu - FM Towns series'
WHERE name = 'Fujitsu - FM-Towns' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Fujitsu - FM Towns series');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Arcade - Incredible Technologies - Eagle'
WHERE name = 'Incredible Technologies - Eagle' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Arcade - Incredible Technologies - Eagle');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Handheld - Mattel - Fisher-Price iXL'
WHERE name = 'Mattel - Fisher-Price iXL' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Handheld - Mattel - Fisher-Price iXL');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Handheld - Palm OS'
WHERE name = 'Palm' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Handheld - Palm OS');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = '3DO Interactive Multiplayer'
WHERE name = 'Panasonic - 3DO Interactive Multiplayer' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = '3DO Interactive Multiplayer');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Datel - PlayStation Cheat Device Updates'
WHERE name = 'PlayStation GameShark Updates' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Datel - PlayStation Cheat Device Updates');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Sega - Prologue 21 Multimedia Karaoke System'
WHERE name = 'Sega - Prologue 21' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Sega - Prologue 21 Multimedia Karaoke System');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Arcade - TAB-Austria - Quizard'
WHERE name = 'TAB-Austria - Quizard' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Arcade - TAB-Austria - Quizard');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'VM Labs - Nuon'
WHERE name = 'VM Labs - NUON' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'VM Labs - Nuon');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Arcade - Funworld - Photo Play'
WHERE name = 'funworld - Photo Play' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Arcade - Funworld - Photo Play');

UPDATE systems
SET url = 'https://redump.info/'
WHERE url = 'http://redump.org/';
