-- Redump moved from redump.org to redump.info and renamed some systems.
-- A renamed system keeps its directory: its old name becomes its custom name,
-- unless it already has one. A rename that would collide with an existing name
-- or custom name is skipped, and that system stays on redump.org.

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Acorn - Archimedes & Risc PC'
WHERE name = 'Acorn - Archimedes' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Acorn - Archimedes & Risc PC')
AND NOT EXISTS (SELECT 1 FROM systems WHERE custom_name = 'Acorn - Archimedes');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Apple - Pippin'
WHERE name = 'Bandai - Pippin' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Apple - Pippin')
AND NOT EXISTS (SELECT 1 FROM systems WHERE custom_name = 'Bandai - Pippin');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Fujitsu - FM Towns series'
WHERE name = 'Fujitsu - FM-Towns' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Fujitsu - FM Towns series')
AND NOT EXISTS (SELECT 1 FROM systems WHERE custom_name = 'Fujitsu - FM-Towns');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Arcade - Incredible Technologies - Eagle'
WHERE name = 'Incredible Technologies - Eagle' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Arcade - Incredible Technologies - Eagle')
AND NOT EXISTS (SELECT 1 FROM systems WHERE custom_name = 'Incredible Technologies - Eagle');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Handheld - Mattel - Fisher-Price iXL'
WHERE name = 'Mattel - Fisher-Price iXL' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Handheld - Mattel - Fisher-Price iXL')
AND NOT EXISTS (SELECT 1 FROM systems WHERE custom_name = 'Mattel - Fisher-Price iXL');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Handheld - Palm OS'
WHERE name = 'Palm' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Handheld - Palm OS')
AND NOT EXISTS (SELECT 1 FROM systems WHERE custom_name = 'Palm');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = '3DO Interactive Multiplayer'
WHERE name = 'Panasonic - 3DO Interactive Multiplayer' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = '3DO Interactive Multiplayer')
AND NOT EXISTS (SELECT 1 FROM systems WHERE custom_name = 'Panasonic - 3DO Interactive Multiplayer');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Datel - PlayStation Cheat Device Updates'
WHERE name = 'PlayStation GameShark Updates' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Datel - PlayStation Cheat Device Updates')
AND NOT EXISTS (SELECT 1 FROM systems WHERE custom_name = 'PlayStation GameShark Updates');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Sega - Prologue 21 Multimedia Karaoke System'
WHERE name = 'Sega - Prologue 21' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Sega - Prologue 21 Multimedia Karaoke System')
AND NOT EXISTS (SELECT 1 FROM systems WHERE custom_name = 'Sega - Prologue 21');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Arcade - TAB-Austria - Quizard'
WHERE name = 'TAB-Austria - Quizard' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Arcade - TAB-Austria - Quizard')
AND NOT EXISTS (SELECT 1 FROM systems WHERE custom_name = 'TAB-Austria - Quizard');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'VM Labs - Nuon'
WHERE name = 'VM Labs - NUON' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'VM Labs - Nuon')
AND NOT EXISTS (SELECT 1 FROM systems WHERE custom_name = 'VM Labs - NUON');

UPDATE systems
SET custom_name = COALESCE(custom_name, name), name = 'Arcade - Funworld - Photo Play'
WHERE name = 'funworld - Photo Play' AND url = 'http://redump.org/'
AND NOT EXISTS (SELECT 1 FROM systems WHERE name = 'Arcade - Funworld - Photo Play')
AND NOT EXISTS (SELECT 1 FROM systems WHERE custom_name = 'funworld - Photo Play');

UPDATE systems
SET url = 'https://redump.info/'
WHERE url = 'http://redump.org/'
AND name NOT IN (
    'Acorn - Archimedes',
    'Bandai - Pippin',
    'Fujitsu - FM-Towns',
    'Incredible Technologies - Eagle',
    'Mattel - Fisher-Price iXL',
    'Palm',
    'Panasonic - 3DO Interactive Multiplayer',
    'PlayStation GameShark Updates',
    'Sega - Prologue 21',
    'TAB-Austria - Quizard',
    'VM Labs - NUON',
    'funworld - Photo Play'
);
