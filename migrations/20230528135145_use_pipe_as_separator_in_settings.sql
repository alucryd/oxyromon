UPDATE
    settings
SET
    value = (
        SELECT
            REPLACE(value, ',', '|')
        FROM
            settings
        WHERE
            key = 'DISCARD_FLAGS'
    )
WHERE
    key = 'DISCARD_FLAGS';

UPDATE
    settings
SET
    value = (
        SELECT
            REPLACE(value, ',', '|')
        FROM
            settings
        WHERE
            key = 'DISCARD_RELEASES'
    )
WHERE
    key = 'DISCARD_RELEASES';

UPDATE
    settings
SET
    value = (
        SELECT
            REPLACE(value, ',', '|')
        FROM
            settings
        WHERE
            key = 'LANGUAGES'
    )
WHERE
    key = 'LANGUAGES';

UPDATE
    settings
SET
    value = (
        SELECT
            REPLACE(value, ',', '|')
        FROM
            settings
        WHERE
            key = 'REGIONS_ALL'
    )
WHERE
    key = 'REGIONS_ALL';

UPDATE
    settings
SET
    value = (
        SELECT
            REPLACE(value, ',', '|')
        FROM
            settings
        WHERE
            key = 'REGIONS_ONE'
    )
WHERE
    key = 'REGIONS_ONE';
