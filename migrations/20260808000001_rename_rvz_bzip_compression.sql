-- The RVZ compression algorithm was stored as 'bzip', which dolphin-tool never
-- accepted: it only knows 'bzip2'. Nothing could have been converted with the
-- old value, and leaving it in place would now fail to parse, so rewrite it.
UPDATE settings
SET value = 'bzip2'
WHERE key = 'RVZ_COMPRESSION_ALGORITHM'
  AND value = 'bzip';
