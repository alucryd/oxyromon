UPDATE romfiles
SET path = CASE
    WHEN path LIKE (SELECT concat(value, '/%') FROM settings WHERE key = 'ROM_DIRECTORY')
    THEN substr(path, length((SELECT rtrim(value, '/') FROM settings WHERE key = 'ROM_DIRECTORY')) + 2)
    ELSE path
END;
