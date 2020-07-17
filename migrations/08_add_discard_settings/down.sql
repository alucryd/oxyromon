DELETE
FROM settings
WHERE "key" IN ('DISCARD_BETA',
                'DISCARD_DEBUG',
                'DISCARD_DEMO',
                'DISCARD_PROGRAM',
                'DISCARD_PROTO',
                'DISCARD_SAMPLE',
                'DISCARD_SEGA_CHANNEL',
                'DISCARD_VIRTUAL_CONSOLE');
