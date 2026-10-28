-- A ROM named without an extension, as a NUS title's contents are, is in
-- its original format unless it is a disk, which is a CHD.
UPDATE roms
SET original = true
WHERE original = false
AND disk = false
AND name NOT LIKE '%.%';
