# 0.1.0

## Features

- Added CIA (CTR Importable Archive) parsing: the header, the section layout with its 64-byte alignment rules, and the title metadata's content records with their offsets into the packed content section
- Added plain extraction, byte-for-byte what ctrtool's `--plain` mode writes: certificates, ticket, title metadata and contents, undecrypted, so extracted contents hash the way DATs recorded them
- Added TMD CMD group hash verification, so a corrupt content index is rejected rather than unpacked as garbage
- Added SimpleCIA (homebrew, format version 0xFF) support, unpacked as a single content
- Added a progress callback to the library API, reporting input bytes consumed, seek gaps included
- Added writing parts as `<name>.part` and renaming once every one is complete, so a failed run leaves an existing set alone and nothing else behind
- Added the `ciars` CLI, in oxyROMon's look, behind the default `cli` feature

## Improvements

- The TMD's signature may be any of the six known ES types and a title of any family keeps its content list, where the reference accepts only RSA-2048/SHA-256 signatures and refuses non-CTR title types; HMAC signatures and unknown types are still named and refused
- A malformed CIA — a truncated section, a lying group hash, a content extending past the container, a missing ticket or certificate chain — comes back as an error naming what is wrong, where the reference read out of bounds or crashed
