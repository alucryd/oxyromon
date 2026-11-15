-- LZMA2 for a cartridge's ROM, Zstandard for a disc, unless set otherwise
UPDATE settings SET value = 'auto'
WHERE "key" = 'ORE_COMPRESSION_ALGORITHM' AND value = 'zstd';
