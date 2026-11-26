# ORE 1.0 — the specification

**ORE**, oxyROMon's ROM Enclosure: one file holding one disc, cartridge ROM
or eShop title, bit-exact, deduplicated in content-defined chunks and
compressed in groups, readable at any offset without decoding the rest; and
the store a library of them can share their chunks in.

| | |
| --- | --- |
| Version | 1.0, `spec_version` 1, magic `ORE1` |
| Date | 2026-11-19 |
| Reference implementation | ore-rs, `crates/ore-rs` of [oxyROMon](https://github.com/alucryd/oxyromon) |
| Conformance vectors | [`spec/vectors`](spec/vectors), §16 |
| License | this text [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/); the vectors [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) |

An ORE written by ore-rs since this version reads in every later reader of
this layout (§14). The key words MUST, MUST NOT, SHOULD and MAY are as
RFC 2119 has them. A *reader* turns an ORE back into what it was written
from; a *writer* makes one. Where this document and ore-rs disagree, it is
a bug in one of them, to be reported.

## Contents

1. Conventions
2. The file
3. The header
4. Groups
5. Codecs
6. The index
7. The metadata
8. Areas and how they read
9. Generators
10. The logical stream, identity and outputs
11. Media classes
12. Stores
13. Limits a reader enforces
14. Compatibility
15. Reserved for later
16. Conformance vectors
- Appendix A: the GameCube and Wii padding generator
- Appendix B: CD sectors
- Appendix C: a store segment's parity
- Appendix D: the range coder
- Appendix E: `xa`
- Appendix F: `vag`
- Appendix G: `str`

## 1. Conventions

- **Integers** in the container's structures are big endian, of the width
  given. `uvarint` is unsigned LEB128, at most 10 bytes and 64 bits; a
  writer MUST write the shortest form, as lengths are computed from it
  (§4). `zigzag` maps a signed integer `n` to `(n << 1) ^ (n >> 63)`.
- **CRC-32C** is CRC-32 of the Castagnoli polynomial (iSCSI, `0x1EDC6F41`,
  reflected, initial and final xor `0xFFFFFFFF`): its check value for the
  ASCII bytes `123456789` is `0xE3069283`. Stored big endian.
- **BLAKE3** is BLAKE3 in its default hashing mode, 32 bytes of output.
- **CBOR** is RFC 8949. §7 has what the metadata holds.
- **Payloads keep their own byte order**: a cartridge's ROM, a disc's
  sectors and CD audio's little-endian PCM are stored as they are.
- An offset in a region (an area, a layer, a group) is from that region's
  start; an offset in the file, from the file's.

## 2. The file

```
 offset 0          header, 192 bytes or more (§3)
 header_len        groups, each an envelope and its stored bytes, end to end (§4)
 index_offset      the index: records, then the group table (§6)
 meta_offset       the metadata, CBOR (§7)
                   anything after, which readers ignore
```

The index and metadata are found by their offsets in the header, so they
may lie anywhere after the groups; ore-rs writes the index right after the
last group and the metadata right after the index. A *thin* ORE (flag
`REQ_LIBRARY`) has no groups: its chunks lie in a store (§12).

## 3. The header

At offset 0. Bytes this table leaves out, and every field marked reserved,
are written zero and ignored by readers.

| Offset | Size | Field | |
| --- | --- | --- | --- |
| 0 | 4 | `magic` | `4F 52 45 31`, `ORE1` |
| 4 | 2 | `spec_version` | 1 for this document |
| 6 | 2 | `header_len` | 192; where the first group starts |
| 8 | 4 | `flags_required` | §3.1 |
| 12 | 4 | `flags_optional` | §3.1 |
| 16 | 1 | `profile` | 0 `stream`, 1 `archive`: informational |
| 17 | 1 | `fidelity` | 0, bit-exact |
| 18 | 2 | `media_class` | §11 |
| 20 | 1 | `index_kind` | 0 flat, 2 thin |
| 24 | 8 | `logical_size` | the logical stream's length, §10 |
| 32 | 4 | `chunk_nominal` | 0: chunks are of variable length |
| 36 | 4 | `chunk_max` | the longest record, informational |
| 40 | 4 | `chunk_count` | the index's records, every area's |
| 44 | 4 | `object_count` | 1 |
| 48 | 8 | `index_offset` | |
| 56 | 8 | `index_len` | |
| 64 | 4 | `index_crc` | CRC-32C of the index |
| 72 | 8 | `meta_offset` | |
| 80 | 4 | `meta_len` | |
| 84 | 4 | `meta_crc` | CRC-32C of the metadata |
| 88 | 32 | `logical_hash` | BLAKE3 of the logical stream |
| 120 | 32 | `merkle_root` | reserved, §15 |
| 152 | 32 | `fixity_offset`, `_len`, `_crc`, `aux_offset`, `_len`, `_crc` | reserved, §15 |
| 184 | 8 | `params_digest` | the first 8 bytes of BLAKE3 of the CBOR of `provenance.params`, informational |

A reader:

- MUST refuse a file whose magic is `ORE` and a digit above `1`, as of a
  later layout, naming it, and treat any other magic but `ORE1` as not an
  ORE.
- MUST NOT refuse a `spec_version` above 1 (§14).
- MUST refuse a `header_len` under 192, and otherwise skip the bytes past
  192: groups start at `header_len`.
- MUST refuse an unknown bit of `flags_required`, and ignore unknown bits of
  `flags_optional`.
- MUST refuse a `media_class` it does not know, an `object_count` other than
  1, and an `index_kind` other than 2 for a thin ORE and 0 otherwise.
- MUST refuse a metadata whose `class` is not the header's `media_class`,
  §13.

### 3.1 Flags

| Bit | `flags_required` | |
| --- | --- | --- |
| 0 | `REQ_VARIABLE_CHUNKS` | records carry their lengths; set by every writer of 1.0 |
| 1 | `REQ_LIBRARY` | thin: the chunks lie in the store `meta.store` names (§12) |
| 2-4 | reserved | §15 |
| 5 | `REQ_SOLID_GROUPS` | chunks are compressed in groups (§4); set on every standalone ORE |

No bit of `flags_optional` is defined in 1.0; ore-rs writes it 0.

## 4. Groups

A standalone ORE's chunks are gathered, in the order their `CHUNK` records
come (§6), into groups of up to 16 MiB, each compressed on its own with one
codec. A group lies in the file as an envelope, then its stored bytes:

```
2   magic            4F 43, "OC"
1   kind             7
1   codec            the codec id (§5) << 4; the low nibble is 0
uvarint  logical_offset   the group's start in the chunks' stream: the sum of
                          the logical lengths of the groups before it
uvarint  stored_len       its stored bytes, which follow
4   crc              CRC-32C of what the group decodes to
```

The groups lie end to end from `header_len`, in the order of the group table
(§6), so a reader finds each from the table alone: a group's envelope starts
where the one before it ends, and its length is `8` plus the lengths of its
two uvarints, written as §1 requires. A store's groups (§12) take the same
envelope, with `logical_offset` 0.

A reader MUST check that the envelope's magic, kind and codec byte are as
the group table says, and that the CRC-32C of the decoded group is the
envelope's; it MAY ignore the envelope's uvarints, which let a damaged
index be rebuilt from the groups alone. The groups MUST end at or before
`index_offset`.

A group decodes to exactly its `logical_len` bytes, or it is corrupt. Its
`stored_len` MUST be at least `logical_len / 1024`, rounded down, and at
most 2²⁸: a decompression bomb is refused before it is decoded. A writer
pads a group that codes to less, as its codec allows (§5).

## 5. Codecs

| Id | Codec | A group is |
| --- | --- | --- |
| 2 | `zstd` | a Zstandard frame |
| 6 | `flac` | FLAC frames of CD audio |
| 8 | `lzma2` | an LZMA2 stream |
| 9 | `xa` | CD-ROM XA ADPCM records, lossless |
| 10 | `str` | STR video sectors' data, lossless |
| 11 | `vag` | SPU ADPCM blocks, lossless |
| 12 | `pcm` | 16-bit PCM as FLAC frames |

Any other id is refused where a group of it is met. `0`, `1`, `3`, `4`, `5`
and `7` are reserved for the draft's `none`, `deflate`, `lz4`, `lzma`,
`brotli` and `avhu`, `13` and up for later codecs.

The metadata names codecs by these names, in lowercase. The parameters a
writer picks (ore-rs: Zstandard level 19, LZMA2 preset 9, FLAC level 8,
each with a window or dictionary of the group's size) change the stored
bytes only; a decoder needs none of them but what follows.

- **`zstd`**: one Zstandard frame (RFC 8878) holding the group, without its
  checksum or with it, then nothing, or skippable frames, which a writer
  pads with (magic `0x184D2A50`). A writer keeps its window to the group's
  size, 16 MiB at most.
- **`lzma2`**: a raw LZMA2 stream, as the xz format's LZMA2 filter holds it:
  chunks, then the end marker `0x00`, then nothing, or zeros, which a writer
  pads with. The dictionary size is not stored: as a match reaches back no
  further than the group's start, a decoder's dictionary of
  2^max(12, ⌈log₂ `logical_len`⌉) bytes suffices.
- **`flac`**: the group is 16-bit stereo little-endian PCM, CD audio, its
  `logical_len` a multiple of 4. It is stored as FLAC frames (RFC 9639)
  alone, without the `fLaC` marker or any metadata block: 2 channels,
  16 bits, 44,100 Hz, blocks of 4,704 samples, 8 CD sectors, the last
  shorter. Each frame's header says all a decoder needs. The frames' samples,
  interleaved left then right, little endian, are the group. Zeros may
  follow the last frame, which a writer pads with.
- **`pcm`**: the group is 16-bit little-endian PCM of a PlayStation 2
  stream. Its first stored byte is its layout: 0 mono, 1 stereo interleaved
  a sample at a time, 2 stereo interleaved 512 bytes a channel. Let `period`
  be 2, 4 or 1,024 bytes, and `whole` the group's `logical_len` rounded down
  to a multiple of it. FLAC frames as `flac`'s follow, of 1 channel for
  layout 0 and 2 otherwise, holding `whole / 2` samples: for layout 2, each
  1,024-byte period holds 256 samples of the first channel then 256 of the
  second, and the frames take them a pair at a time, sample `i` of the first
  then sample `i` of the second. Then come the `logical_len − whole` bytes
  past the last whole period, as they are, then zeros, which a writer pads
  with.
- **`xa`**, **`vag`**, **`str`**: lossless models of PlayStation audio and
  video, each an adaptive binary range coder over what the group's format
  holds, every byte kept, so that any bytes go through. A group of `xa` is
  records of 2,328 bytes, each a sector's first subheader copy and its
  2,324 bytes of data (§8.4); of `vag`, 16-byte SPU ADPCM blocks, a stream's
  48-byte `VAGp` header included; of `str`, video sectors' data, 2,048
  bytes a Form 1 sector, 2,324 a Form 2. Appendices D to G define them.

A writer MAY store any group with `zstd` or `lzma2` instead of the codec its
area suggests (§7): ore-rs does when Zstandard codes a group of audio
smaller, as when a disc flags data as XA audio.

## 6. The index

`index_len` bytes at `index_offset`, its CRC-32C `index_crc`. First the
records, every area's in turn, in the order `meta.media.areas` lists the
areas, each area taking its `records` (§7); then the group table. Nothing
may follow the group table.

### 6.1 Records

A record covers the next `len` bytes of its area's *stored stream* (§8), in
order, from 0:

```
1        kind
uvarint  len              1 to 16 MiB
         what the kind adds
```

| Kind | Name | Adds | Its bytes |
| --- | --- | --- | --- |
| 2 | `ZERO` | nothing | `len` zeros |
| 4 | `GENERATED` | `gen:u8`, then what the generator takes | what generator `gen` produces (§9) |
| 5 | `DEDUP` | `target:uvarint` | the stored chunk `target`, again |
| 7 | `CHUNK` | nothing | the next stored chunk |
| 9 | `STORE` | `delta:uvarint` | the store's chunk `id` (§12) |

The *stored chunks* are the `CHUNK` records' chunks, numbered from 0 in the
order their records come; each is the next `len` bytes of the groups' decoded
stream, in group order (§6.2). A `DEDUP` record names one already numbered:
`target` MUST be below the count of `CHUNK` records before it, and `len`
that chunk's length. A `STORE` record's `delta` is `zigzag` of its id less
the id of the `STORE` record before it in the index, or of 0 for the first.

A standalone ORE takes kinds 2, 4, 5 and 7; a thin one, 2, 4 and 9. Any other
kind, or one of the other sort of ORE, is refused. Kinds 0, 1, 3, 6 and 8
are reserved for the draft's `RAW`, `CODEC`, `FILL`, `PARENT` and
`PLACEHOLDER`, 10 and up for later ones. Kind 7 was the draft's `SOLID`,
whose sub-chunks the group table now gives.

An area's records MUST add up to its stored stream's length exactly, and the
records of all areas to `chunk_count`.

### 6.2 The group table

```
uvarint  count
count times:
  1        codec            its id (§5), not shifted
  uvarint  chunks           how many stored chunks it holds, in order
  uvarint  logical_len      at most 16 MiB
  uvarint  stored_len
```

The stored chunks fill the groups in order: group 0 holds the first `chunks`
of them, and so on. A group's chunks' lengths MUST add up to its
`logical_len`, and the groups MUST hold every stored chunk, and no more. A
thin ORE's `count` is 0.

## 7. The metadata

`meta_len` bytes at `meta_offset`, at most 16 MiB, its CRC-32C `meta_crc`:
one CBOR map. Maps have text keys; integers are unsigned; byte strings are
CBOR byte strings; lengths are definite.

A reader MUST ignore keys it does not know, which is how later versions add
to it (§14), and MUST refuse a value it does not know where this section
gives the values a field takes, an area kind or a codec, say. It MUST NOT
depend on the order of keys: OREs written before ore-rs wrote them in
canonical order are 1.0 files too. A writer SHOULD write the metadata in
RFC 8949 §4.2.1 core deterministic encoding, as ore-rs does from this
change on, so that two encodes of the same input are the same bytes:
integers in their shortest form, lengths definite, no tags, and each
map's keys in bytewise order, which for text keys puts the shorter key
first. A writer leaves out an optional key without a value. The listings
below give the keys in that order.

Below, `?` marks an optional key, `[ ]` an array.

```
meta = {
  media:      media,
  ? store:    { uuid: bytes(16) },        ; a thin ORE's store, §12
  ledger:     [ step ],
  identity:   [ identity ],
  provenance: provenance,
}

media = {
  ? cue:      bytes,                      ; a CD's cue sheet, as its dump has it
  ? sbi:      bytes,                      ; a CD's SBI, as its dump has it
  size:       uint,                       ; = logical_size
  areas:      [ area ],                   ; in the order their records come
  class:      uint,                       ; must equal the header's media_class, §13
  ? ticket:   { name: text, bytes: bytes },   ; an eShop title's ticket, a file of its folder
  platform:   text,                       ; §11
}

area = {
  len:          uint,                     ; bytes of the logical stream it covers, > 0
  ? file:       text,                     ; the file it is, or part of, §10
  kind:         "raw" / "wii-partition" / "cd-data" / "cd-audio" / "wiiu" / "repeat",
  ? wiiu:       { ? outer: layer, ? pieces: [ { len: uint, layer: layer, offset: uint } ] },
  ? codec:      codec name,               ; what its groups were meant for, informational
  ? frame:      uint,                     ; a CD track's first sector, frames from 00:00:00
  start:        uint,                     ; where it starts in the logical stream
  ? period:     uint,                     ; a repeat's, §8.7
  records:      uint,                     ; the index records it takes
  ? sectors:    bytes,                    ; a CD data track's sector map, §8.4
  ? title_key:  bytes(16),                ; a Wii partition's, decrypted
  ? exceptions: [ { block: uint, hashes: bytes(1024) } ],   ; a Wii partition's, §8.5
}

layer = { ? ivs: bytes, key: bytes(16), mode: "chain" / "blocks" / "hashed",
          index: uint (< 65536) }

step       = { id: text, ? bytes: uint, verified: bool }
identity   = { algo: text, view: text, value: bytes }
provenance = { tool: text, params: { codec: codec name, group: uint, level: uint,
               chunker: { max: uint, min: uint, name: text } } }
```

`ledger` and `provenance` are informational: what the writer did to the
input, and with what. A reader MUST NOT depend on either but to parse them;
ore-rs's steps are `wii-decrypt`, `wii-hashes`, `gc-wii-filler-prng`, `zero`,
`fill`, `mincdc-dedup`, `cd-sectors`, `wiiu-decrypt` and `wiiu-repeat`, each
with the bytes it accounts for. `identity` is §10's.

## 8. Areas and how they read

The areas tile the logical stream: sorted by `start`, the first starts at
0, each where the one before it ends, the last at `logical_size`, none
empty. They are listed in the order their records come, which may differ:
a PlayStation 2 DVD's audio streams come after the rest of it.

Each area has a *stored stream*, which its records cover in order (§6.1),
and its bytes of logical stream are made from it as its kind says:

| Kind | Stored stream | Logical bytes |
| --- | --- | --- |
| `raw` | `len` bytes | the stored stream |
| `cd-audio` | `len` bytes | the stored stream: a CD track's PCM |
| `cd-data` | its sector map's planes (§8.4) | the track's sectors rebuilt |
| `wii-partition` | `len / 0x8000 × 0x7C00` bytes | the partition's blocks hashed and encrypted (§8.5) |
| `wiiu` | `len` bytes | the stored stream encrypted by its layers (§8.6) |
| `repeat` | none | the logical stream `period` bytes before (§8.7) |

A reader MUST refuse an area missing what its kind needs: `cd-data` its
`sectors` and `frame`, `cd-audio` its `frame`, `wii-partition` its
`title_key`, `wiiu` its `wiiu`, `repeat` its `period`.

### 8.1 `raw`

The stored stream is the logical bytes. A cartridge's ROM, a disc image's
bytes outside its partitions, a title's files kept as they are.

### 8.2 `cd-audio`

A CD audio track, its `len` a multiple of 2,352, its `frame` at most
2³¹ − 1: its 16-bit little-endian stereo PCM, as its BIN holds it. Its
groups are usually `flac`.

### 8.3 Sectors and frames

A CD track's sector `i` lies at frame `frame + i`, frames counted from
00:00:00 at 75 a second, so that a disc's first sector, at LBA 0, is frame
150. A GD-ROM's tracks count their frames the same way, its high-density
area starting at LBA 45,000, frame 45,150.

### 8.4 `cd-data`

A CD data track, its `len` a multiple of 2,352, its `frame` at most
2³¹ − 1. Only what cannot be generated of its sectors is stored, in three
planes, and its `sectors` map says what each sector is.

**The map** is Zstandard compressed. Decompressed, it is for each sector, in
order, one byte, `kind | DIFFERS (0x80) | VIDEO (0x40)`, then, for a kind
with a subheader, the subheader's first 4-byte copy, and its second when
`DIFFERS` is set:

| Kind | Sector | Data bytes | Subheader |
| --- | --- | --- | --- |
| 0 | raw: kept whole | 2,352 | no |
| 1 | all zeros | 0 | no |
| 2 | Mode 1 | 2,048 | no |
| 3 | Mode 2 Form 1 | 2,048 | yes |
| 4 | Mode 2 Form 2 | 2,324 | yes |
| 5 | Mode 2 Form 2, its EDC zero | 2,324 | yes |

Without `DIFFERS`, the subheader's second copy is its first. A reader MUST
refuse a kind above 5, `DIFFERS` or `VIDEO` on a sector without a
subheader, and a map whose sectors are not the track's, `len / 2,352`; it
MUST bound what it decompresses by 9 bytes a sector before allocating it.

**The planes.** Each sector's data goes in one plane:

- *video*, if `VIDEO` is set: STR video, its data, 2,048 or 2,324 bytes as
  its kind has;
- else *XA*, if it is of kind 4 or 5 and its subheader's third byte has bit
  `0x04` set and its fourth byte has neither bit of `0x30`: XA ADPCM, a
  record of 2,328 bytes, its subheader's first copy then its 2,324 bytes of
  data;
- else *data*: its data bytes, as many as its kind has.

The stored stream is the data plane, then the video plane, then the XA
plane, each its sectors' records in the order of the sectors. A reader takes
an XA sector's subheader from the map, its record's first 4 bytes being
only what the `xa` codec models. The planes are usually stored in groups of
`zstd`, `str` and `xa`, in that order.

**A sector is rebuilt** from its kind, its frame, its subheader and its
data as Appendix B says.

### 8.5 `wii-partition`

A Wii partition's data area, from its first block, its `len` a multiple of
0x8000: 32 KiB blocks, each 0x400 bytes of hashes then 0x7C00 of data, as
the disc holds them encrypted with the partition's title key, `title_key`,
stored decrypted. The stored stream is the blocks' data alone, 0x7C00 bytes
a block, in order; the hashes are recomputed.

The blocks are in groups of 64, 2 MiB, the last group perhaps shorter. For
each group, its blocks' data, with zero blocks for the ones a last group
lacks, is hashed with SHA-1 as a Wii disc does:

- each block's H0 table, at 0x000 of its hashes, is the SHA-1 of each of
  its 31 KiB of data, 20 bytes each, 31 of them;
- each block's H1 table, at 0x280, is the SHA-1 of the H0 tables of the 8
  blocks of its eighth of the group, in order, the same in all 8;
- each block's H2 table, at 0x340, is the SHA-1 of the H1 table of each of
  the group's 8 eighths, in order, the same in all 64;
- the rest of its hashes, at 0x26C, 0x320 and 0x3E0, is zeros.

Then each block whose number counted from the partition's first block,
`group × 64 + j`, an `exceptions` entry names takes that entry's 1,024
bytes instead, as the disc has them decrypted. Each block is then
encrypted with AES-128-CBC and the title key: its hashes from an IV of
zeros, then its data from the IV that is bytes 0x3D0 to 0x3E0 of its
encrypted hashes.

### 8.6 `wiiu`

A region of a Wii U disc, or an eShop title's content, its `len` a multiple
of 16, stored decrypted: the stored stream is its plaintext, and its
logical bytes are that plaintext encrypted by the region's layers, first
each piece's, then the outer layer's.

- A **piece** encrypts its `len` bytes from `offset` in the region with its
  layer, the piece's first byte being its layer's byte 0. Pieces are sorted
  by offset, do not overlap, lie within the region, start on a multiple of
  16 and are not empty; their mode is `chain` or `hashed`.
- The **outer** layer, if any, encrypts the whole region, after the pieces.

Each layer is AES-128-CBC with its `key`, over `n` bytes, a multiple of 16,
in one of three modes. Let `iv(k)` be, for `k` = 0, 16 bytes of zeros with
`index` big endian in the first two; for `k` > 0, 16 bytes of zeros with
`k` big endian in the last eight.

- **`chain`**: one CBC chain from `iv(0)`. `ivs` holds 16 bytes for each
  64 KiB boundary within the layer, `⌈n / 0x10000⌉ − 1` of them: the
  ciphertext's 16 bytes before it, so that a reader encrypts from any
  boundary, taking it as the IV there. A reader MUST refuse `ivs` of any
  other length, and any `ivs` on a layer of another mode.
- **`blocks`**: each 64 KiB block `k` of the layer is a chain of its own
  from `iv(k)`. Outer layers only.
- **`hashed`**: `n` is a multiple of 0x10000. Each 64 KiB block `k` of the
  layer is 0x400 bytes of hashes, encrypted from an IV of zeros, then 0xFC00
  of data, encrypted from the IV that is the 16 bytes at `20 × (k mod 16)`
  of its plaintext hashes, which are stored, as the disc has them, with
  its data.

`index` is the content's index in its title, or the cluster's on the disc,
as the title or partition gives it.

### 8.7 `repeat`

A Wii U disc's mastering fills a partition's free space with the region
before it. A `repeat` area's logical bytes are the logical stream's
`period` bytes before them, read from what comes before, perhaps from
another `repeat`: byte `x` of the area is byte `start + x − period` of the
logical stream. `period` MUST be above 0, a multiple of 0x8000, and at most
`start`, and the area takes no records.

## 9. Generators

A `GENERATED` record names its generator with a byte, then what it takes:

| Id | Generator | Takes | Produces |
| --- | --- | --- | --- |
| 4 | `gc-wii-filler-prng` | `seed`, 68 bytes | the padding of a GameCube or Wii disc (Appendix A) |
| 5 | `fill` | `byte`, 1 byte | `len` copies of `byte`, as a ROM's padding |

Any other id is refused. 0 to 3 and 6 are reserved for the draft's
`cd-edc`, `cd-ecc-c22`, `cd-sync-header`, `pattern-run` and
`zero-fill-tail`, 7 and up for later ones. A generator is never changed in
place: a new one takes a new id.

**`gc-wii-filler-prng`** restarts at each record. The record lying at
offset `at` of its area's stored stream produces, from its byte `x`, the
generator's output for `seed` from offset `((phase + at) mod 0x8000) + x`,
where `phase` is the area's `start` for a `raw` area and 0 for any other:
the padding counts from the disc's 32 KiB blocks outside a Wii partition,
and from its data's within one.

## 10. The logical stream, identity and outputs

The **logical stream** is the bytes the areas make, from 0 to
`logical_size`: a disc's image as its DAT lists it; a cartridge's ROM; a
CD's tracks' BIN files, end to end in the order its cue sheet names them;
an eShop title's files, end to end in the order of their names.
`logical_hash` is its BLAKE3. A reader that reads it all MUST check it, and
report a mismatch as damage.

**Identity** lists the hashes a DAT knows the input by, recorded as it was
written: each an `algo`, `blake3-256`, `sha-1`, `md5` or `crc-32` (ISO-HDLC,
the zlib one, 4 bytes big endian), and a `view`, what it covers: `logical`,
the logical stream, or a file's name, for each file of a CD (its tracks, its
cue sheet, its SBI) or of a title (its files, its ticket). The `blake3-256`
of the `logical` view is `logical_hash`. A stored hash is a claim: a check
that matters recomputes it.

**Outputs.** What a reader writes the logical stream back out as:

- a disc image or a ROM, the logical stream as one file; a cartridge's ROM
  SHOULD take its first area's `file` as its name;
- a CD or GD-ROM, which has a `cue`: the cue sheet, `cue`'s bytes; each area
  a track's file named `file`, beside the cue sheet, its bytes from `start`
  to `start + len`; and if there is an `sbi`, its bytes, named as the cue
  sheet with the extension `.sbi`;
- an eShop title, of class 97: a folder of its files, each the areas, by
  `start`, that run on with the same `file`, from the first's `start` for
  their lengths together; and its `ticket`, `bytes` under `name`.

A reader MUST refuse to write a file whose name is not a bare file name,
holding no directory, or that another file of the same output takes.

## 11. Media classes

| Id | Class | What | `platform` | Areas |
| --- | --- | --- | --- | --- |
| 16 | `rom-flat` | a file taken whole, as a DAT hashes it: a floppy's image | §11.1 | `raw` |
| 17 | `rom-cartridge` | a cartridge's ROM, as its dump holds it | §11.1 | `raw` |
| 32 | `cd-2352` | a CD of data tracks, 2,352-byte sectors | `cd` | `cd-data` |
| 33 | `cd-mixed` | a CD with audio tracks | `cd` | `cd-data`, `cd-audio` |
| 48 | `gd-rom` | a GD-ROM: a CD's single-density area and its high-density area | `gd-rom` | `cd-data`, `cd-audio` |
| 64 | `dvd-2048` | a disc image of 2,048-byte sectors, or a Wii U disc's of 32 KiB | `gamecube`, `wii`, `ps2`, `psp`, `wiiu` | `raw`, `wii-partition`, `wiiu`, `repeat` |
| 97 | `cdn-title` | an eShop title as its CDN delivers it: a Wii U NUS folder | `wiiu` | `raw`, `wiiu` |

The class tells a reader what an emulator is handed: sectors of 2,352 bytes
with a cue sheet's tracks, an image, a ROM, or a folder of files. Ids the
draft's registry names and 1.0 does not build are reserved: 0 `opaque`, 49 `gdr1`, 65 `dvd-raw-2352`, 80 `bd-2048`, 96 `gamecard`, 112
`nand`, 128-130 `hdd-512`, `hdd-4096`, `mo`, 144 `av-frames`, 160
`folder`, 240 `flux-track`, 241 `tape-pulse`.

### 11.1 Platforms

`platform` names the system, for a reader to tell an emulator; it does not
change how the ORE decodes. A cartridge's is one of `32x`, `3ds`, `a2600`,
`a5200`, `a7800`, `colecovision`, `dsi`, `gb`, `gba`, `gbc`, `gg`, `jaguar`,
`lynx`, `md`, `n64`, `nds`, `nes`, `ngp`, `ngpc`, `odyssey2`, `pce`,
`pokemini`, `satellaview`, `sg1000`, `sgx`, `sms`, `snes`, `sufami`, `vb`,
`ws` and `wsc`; a floppy's, of class 16, one of `amiga`, `fds`, `n64dd` and `st`,
the image an FDS or a 64DD disk's, its BIOS's, or an IPF as the DAT lists it. A reader SHOULD accept a platform it does not know.

## 12. Stores

A store holds the chunks of many thin OREs, each kept once: a folder named
`.ore-store`, holding `store.index`, its copy `store.index.copy`, its
segments, and `store.lock`, which writers lock. A thin ORE finds its store
as the nearest `.ore-store` up from its own folder, its own folder's first:
a system's folder holds its store, and every folder beneath it finds it.
The store's index MUST have the uuid `meta.store` names, or it is another
store.

### 12.1 `store.index`

A reader reads `store.index`, or `store.index.copy` if the former is missing
or unreadable; a writer replaces each whole, the index first.

```
4   magic          "ORSI"
2   version        2, or 1 (below)
1   id_bits        32 or 64: chunk ids' width; at most 2^id_bits chunks
1   parity         parity shards new segments are given, 0 to 16
16  uuid
8   segment_size   what a writer grows a segment to, informational
4   next_segment   the number the next segment takes
4   segments       their count
8   groups         their count, under 2^32 − 1
8   chunks         their count
then segments × 13: number:u32, len:u64, parity:u8
then groups × 21:   segment:u32, offset:u64, codec:u8, logical_len:u32, stored_len:u32
then chunks × 32:   digest:16, group:u32, offset:u32, len:u32, refs:u32
4   CRC-32C of all the bytes before it
```

The tables MUST add up to the index's length. Version 1's segment records
are 12 bytes, without `parity`: each segment takes the header's. A segment's
number is below `next_segment`, and no two share it. A group lies in its
segment from `offset`, at least 32, its envelope and stored bytes ending
within the segment's `len`, its `logical_len` at most 16 MiB and its
`stored_len` at least `logical_len / 1024`, as §4's. A chunk is the bytes
from `offset` of its group's decoded bytes, `len` of them, above 0; its
`digest` is the first 16 bytes of the BLAKE3 of those bytes; `refs` counts
the games using it, informational. A chunk whose `group` is `0xFFFFFFFF` is
in no group any more, its id kept for when it comes back.

A thin ORE's `STORE` record names a chunk by its id, its number in the
table. A reader MUST refuse an id the table does not hold, a chunk in no
group, or one whose `len` is not the record's. It MAY check a chunk's bytes
against its `digest` where it reads a game whole, as a check or a
conversion does: a group's CRC already finds damage to its bytes, and the
logical BLAKE3 a whole game's, so a read for an emulator need not.

### 12.2 Segments

Segment `n` is the file `segment-` and `n` in six or more decimal digits,
zero padded, then `.ores`: `segment-000001.ores`. It starts with a 32-byte
header, then its groups, each an envelope and its stored bytes (§4), to its
`len`:

```
4   magic     "ORSS"
2   version   1
2   zero
4   number
4   zero
16  uuid      the store's
```

A reader MUST check the header is exactly this before reading a group of it.
A segment with `parity` above 0 is *sealed*: its parity follows its `len`
bytes (Appendix C). Parity is for checking and repair; reading needs none
of it.

## 13. Limits a reader enforces

A reader MUST refuse, rather than allocate, loop or read past what the file
holds, when:

- a region, the index or the metadata, lies past the end of the file, fails
  its CRC, or, for the metadata, is above 16 MiB;
- the metadata's `size` is not `logical_size`, its `class` is not
  `media_class`, a thin ORE names no store or
  a standalone one names one, or a uuid is not 16 bytes;
- the areas do not tile the logical stream, or one fails §8's rules;
- a record is of an unknown kind or generator, of length 0 or above 16 MiB,
  or names a chunk §6 does not allow;
- an area's records do not add up to its stored stream, or all of them to
  `chunk_count`;
- a group's codec is unknown, its `logical_len` above 16 MiB, its
  `stored_len` above 2²⁸ or below `logical_len / 1024`, its chunks not its
  length, or the groups overlap the index;
- the index holds more than it describes;
- a group's envelope, CRC or decoded length is not what its table says;
- a store's index or segment, or a chunk, fails §12's rules.

It MUST bound what a length from the file makes it allocate by what the file
can hold, as §8.4 does a sector map, and MUST NOT recurse on the metadata.

## 14. Compatibility

What is in a file of this layout reads the same in every later reader;
what a later writer adds, a reader of this layout reads past, unless it
cannot decode without it:

- **A layout break** changes the magic, `ORE1` to `ORE2`, which a reader of
  this layout refuses, naming it. Nothing planned needs one.
- **An addition** raises `spec_version`, which is not a gate: a reader of
  this layout reads a file of a later version.
- **What a reader needs to decode** — a record kind, a generator, a codec, an
  area kind, a media class — is refused where it is met by a reader not
  knowing it, as for a `flags_required` bit.
- **What a reader can do without** sets a `flags_optional` bit or fills a
  region a reader leaves unread: bytes after its last structure, the header
  past 192 bytes or its reserved fields, metadata keys it does not know.

ore-rs's `tests/compat.rs` holds its reader to this: a higher
`spec_version`, an unknown optional flag, filled reserved fields, a longer
header and appended bytes read the same; an unknown required flag, a shorter
header and `ORE2` are refused.

## 15. Reserved for later

The design draft this version was finalized from specifies more than 1.0 builds. These keep their place, so that they
arrive as additions (§14):

- **Fixity**: a BLAKE3 tree over the chunks, its root in `merkle_root`, in
  the region `fixity_*` names; and in-file parity, `OPT_PARITY`. Zero while
  absent.
- **AUX**: non-reproducible provenance, the region `aux_*` names, outside
  identity.
- **Tail index**, `OPT_TAIL_INDEX`: a copy of the index after the rest.
- **The `deep` profile**, 2: informational, as the profile byte is.
- **Sparse indexes**, `index_kind` 1, and **library files**, several objects
  in one, `object_count` above 1.
- **Required flags** 2-4, the draft's `REQ_SEALED`, `REQ_LEDGER_SIGNED` and
  `REQ_PLANES`; **prefilters**, the codec byte's low nibble; **parent
  objects**, `PARENT` records; **key requests**, `meta.keyreq`; **fidelity**
  grades 1 and 2.

## 16. Conformance vectors

[`spec/vectors`](spec/vectors) holds OREs of every media class and record
kind, every codec, a store with sealed segments and the thin OREs that read
through it, and hostile files a reader MUST refuse. Its `manifest.tsv` lists,
for each:

```
vector  <path>  ok      <logical_size>  <logical BLAKE3, hex>  <what it covers>
vector  <path>  refuse  -               -                      <why>
file    <path>  <output file>  <size>  <SHA-1, hex>
```

A reader conforms to 1.0 when it reads every `ok` vector's logical stream to
its size and BLAKE3, writes each of its outputs (§10) to the manifest's
sizes and SHA-1s, and refuses every `refuse` vector. ore-rs's
`tests/vectors.rs` holds it to them. The vectors' inputs are synthetic, made
by ore-rs's tests, with test keys of no console.

## Appendix A: the GameCube and Wii padding generator

A lagged Fibonacci generator, k = 521, j = 32, of 32-bit words: what a
GameCube or Wii disc's mastering fills unused space with, and RVZ stores as
its seed. A port of Dolphin's `LaggedFibonacciGenerator`.

**Seeding.** The seed is 68 bytes, 17 words `s[0..17]`, each big endian.

1. `b[i] = s[i]` for `i` in 0..17.
2. For `i` in 17..521: `b[i] = (b[i−17] << 23) ^ (b[i−16] >> 9) ^ b[i−1]`,
   in 32 bits.
3. Each word `b[i] = (b[i] & 0xFF00FFFF) | ((b[i] >> 2) & 0x00FF0000)`.
4. Step 4 times.

**A step** is, for `i` in 0..32, `b[i] ^= b[i + 489]`; then for `i` in
32..521, `b[i] ^= b[i − 32]`.

**Output.** The generator's output is the 2,084 bytes of `b[0..521]`, each
word big endian, then a step, the next 2,084 bytes, and so on. Its output
from offset `o` skips the first `o` bytes.

## Appendix B: CD sectors

As ECMA-130 lays them out. A sector of kind 0 is its 2,352 bytes of data; of
kind 1, 2,352 zeros. Every other sector starts with the sync, `00`, ten
`FF`, `00`; then its address, its frame `f` as minutes `f / 4500`, seconds
`f / 75 mod 60` and frames `f mod 75`, each a BCD byte; then its mode byte;
its data at the offset its kind gives; and zeros where nothing else is put:

| Kind | Mode byte | Bytes 16-23 | Data | EDC | Then |
| --- | --- | --- | --- | --- | --- |
| 2, Mode 1 | 1 | — | 16, 2,048 bytes | of bytes 0-2063, at 2064 | 8 zeros, then P and Q parity at 2076 |
| 3, Form 1 | 2 | subheader, both copies | 24, 2,048 bytes | of bytes 16-2071, at 2072 | P and Q parity at 2076 |
| 4, Form 2 | 2 | subheader, both copies | 24, 2,324 bytes | of bytes 16-2347, at 2348 | — |
| 5, Form 2 | 2 | subheader, both copies | 24, 2,324 bytes | zero | — |

**The EDC** is a CRC-32 of the reflected polynomial `0xD8018001` (normal
`0x8001801B`), initial value and final xor 0, stored little endian.

**The P and Q parity** are ECMA-130 Annex A's Reed-Solomon product code: P,
172 bytes at 2076, over bytes 12 to 2075, 86 columns of 24; Q, 104 bytes at
2248, over bytes 12 to 2247, P included, 52 diagonals of 43. For a Mode 2
sector, the 4 address and mode bytes are taken as zeros. MAME's
`cdrom_file::ecc_generate` and ore-rs's `crates/cd/src/ecc.rs` compute it.

## Appendix C: a store segment's parity

A sealed segment, of `len` bytes and `p` parity shards, 1 to 16, is followed
by its parity shards, its table of CRCs twice, and a trailer.

**Shards.** Let `shard` = ⌈`len` / 239⌉ rounded up to a multiple of 4,096,
and 4,096 at least. Data shard `i`, for `i` in 0..239, is the segment's bytes
from `i × shard`, `shard` of them, zeros past its end. Parity shard `j`, for
`j` in 0..`p`, is `Σ M[239 + j][i] · data[i]` over GF(2⁸), byte by byte,
where GF(2⁸) is of the polynomial `x⁸ + x⁴ + x³ + x² + 1` (`0x11D`), and
`M = V × T⁻¹`: `V` the `(239 + p) × 239` Vandermonde matrix
`V[r][c] = r^c` (`0^0 = 1`), `T` its top 239 rows. So `M`'s top rows are the
identity, as a systematic code's. This is the construction of
Backblaze's JavaReedSolomon and of the `reed-solomon-erasure` crate.

**Blocks.** The segment's bytes are its *data blocks*, 4,096 bytes from each
multiple of 4,096, the last perhaps shorter; the parity shards' bytes, in
order, are its *parity blocks*, 4,096 bytes each. A *row* is the block at
the same place in every shard: a data shard's block wholly past `len` is
zeros, and not stored.

**The table** is the CRC-32C of each block, big endian, the data blocks' in
order then the parity blocks'; a short last data block's CRC is of its bytes
alone. It is written twice, the second copy right after the first.

**The trailer**, 32 bytes, ends the file:

```
4   magic    "ORSP"
2   version  1
1   p
1   zero
8   len
8   shard
4   zero
4   CRC-32C of the 28 bytes before it
```

A block that fails both tables' CRCs is an erasure in its row, and a row
that has no more of them than `p` is rebuilt by the code.

## Appendix D: the range coder

`xa`, `vag` and `str` code a group with one adaptive binary range coder,
LZMA's, its probabilities each the mean of a fast and a slow estimate. This
appendix and the next three define what a group decodes to; an encoder is
free in its choices wherever the decoder is told them, and conforms when
its output decodes to the group. Integers below are two's complement; `>>`
of a signed integer is arithmetic, rounding towards minus infinity; `/`
rounds towards zero; `clamp(x, a, b)` is `min(max(x, a), b)`.

**A probability** is two 16-bit integers, `fast` and `slow`, both 32,768 at
first: `p = clamp((fast + slow) >> 1, 32, 65,504)` is the chance of a 0, in
65,536ths. Coding bit `b` with it updates it: for a 0, `fast += (65,535 −
fast) >> 4` and `slow += (65,535 − slow) >> 7`; for a 1, `fast −= fast >> 4`
and `slow −= slow >> 7`. Every probability of a model starts afresh with
each group.

**The decoder** has a 32-bit `range`, `0xFFFFFFFF`, and a 32-bit `code`,
the group's first 5 bytes, shifted in one at a time, `code = code << 8 |
byte`, in 32 bits; a byte past the stored bytes is 0. To decode a bit with
probability `p`: `bound = (range >> 16) × p`; if `code < bound`, the bit is
0 and `range = bound`; otherwise it is 1, `code −= bound` and `range −=
bound`. Then `p` is updated, and while `range < 2²⁴`, `range <<= 8` and
`code = code << 8 | next byte`.

**A tree** of `n` bits, an array of `2ⁿ` probabilities, codes a value of
`n` bits high bit first: `node = 1`; `n` times, `node = node × 2 + bit`,
the bit decoded with probability `node`; the value is `node − 2ⁿ`. A
*byte tree* is a tree of 8 bits.

**A number** of a 64-probability array `P`, a value `v` below 2¹⁶, is its
bit length `c` in unary, then its bits below its top one. The unary is, for
`i` = 0, 1, …, a bit of `P[i]`, 1 while `i < c`, ending at its first 0 or
after 16 ones. Then, for `c` above 1, `v`'s `c − 1` bits below its top one,
high bit first, each of `P[32 + c]`. So 0 is `0`, 1 is `1 0`, 5 is `1 1 1 0`
then `0 1`.

The ore-rs encoder is LZMA's: a 64-bit `low`, a cache byte and a count of
pending `FF` bytes, flushed with 5 shifts at the end.

## Appendix E: `xa`

A group is records of 2,328 bytes, a sector's 4-byte subheader then its
2,324 bytes of data, and whatever bytes follow the last whole record.

**The model**, afresh each group: 4 byte trees for the subheader's 4
bytes; 256 byte trees for a sound unit's parameter, by the channel's last;
a probability for the copies; a byte tree for raw bytes; 192 trees of 4
bits for the samples; a probability for the tail. And the channels, by key,
each made when first met: XA's last two samples `old` and `older`, 0;
`last_param`, 0; the predictor's 11 inputs and 11 weights, 0; and `error`,
512.

**The predictor.** Its inputs are the channel's last 8 samples, newest
first, then 3 of the other channel's. It predicts `predicted =
clamp((Σ inputᵢ × weightᵢ) >> 16, −65,536, 65,536)`, in 64 bits. Learning a
sample `s` it predicted: `energy = max(Σ inputᵢ², 2²⁰)`; `factor = ((6,554 ×
(s − predicted)) << 16) / energy`; each `weightᵢ = clamp(weightᵢ + (factor ×
inputᵢ) / 65,536, −2²⁰, 2²⁰)`; then its own 8 inputs shift down, the oldest
dropped, `s` the newest, and `older = old`, `old = s`. Its last 3 inputs
change only as set below.

**A record.** Its 4 subheader bytes, each with its byte tree. A record is
ADPCM when the subheader's third byte has bit `0x04` and its fourth has
neither bit of `0x30`; one that is not is its 2,324 bytes, each with the
raw byte tree. An ADPCM record is stereo when its fourth byte's low 2 bits
are 1; its channels' keys are `byte0 << 16 | byte1 << 8`, plus 1 for a
stereo record's odd sound units, its right channel.

The data is 18 sound groups of 128 bytes, then a tail of 20. A sound
group's bytes 4 to 11 are its 8 units' parameters, bytes 0 to 3 and 12 to
15 their copies, and bytes 16 to 127 its 28 words of 4 bytes, the samples:
unit `u`'s sample `j` the nibble of byte `16 + 4j + u / 2`, the low one for
an even `u`. For each sound group:

1. For each unit `u`, 0 to 7, its parameter, byte `4 + u`, with the
   parameter tree of its channel's `last_param`, which it then becomes.
2. A bit of the copies' probability: 1 when bytes 0-3 are bytes 4-7 and
   bytes 12-15 bytes 8-11, which they are set to; 0, then bytes 0-3 and
   12-15, each with the raw byte tree.
3. For each unit `u`, 0 to 7, its channel `ch`, its parameter's `shift`,
   its low 4 bits, taken as 9 above 12, and `filter`, bits 4-5; then for
   each sample `j`, 0 to 27:
   - for a right channel, its inputs 8 to 10 are `left[min(max(j + d − 1,
     0), 27)]` for `d` = 0, 1, 2;
   - `pred = (ch.old × K0[filter] + ch.older × K1[filter] + 32) >> 6`, with
     `K0 = [0, 60, 115, 98]` and `K1 = [0, 0, −52, −55]`;
   - `predicted` the channel's prediction, `expected = ((predicted − pred)
     << shift) >> 4`;
   - the nibble, with the sample tree `(clamp((expected + 64) >> 7, −16, 15)
     + 16) × 6 + min(ch.error >> 7, 5)`;
   - `t` the nibble sign-extended from 4 bits, the sample `s =
     clamp(((t << 12) >> shift) + pred, −32,768, 32,767)`;
   - `ch.error = (ch.error × 9 + min(|(t << 8) − expected|, 4,096)) / 10`;
   - the channel learns `s` (above); and `left[j] = s` for a mono record or
     an even unit.

After the 18 sound groups, a bit of the tail's probability: 1 when the 20
tail bytes are zeros; 0, then each with the raw byte tree.

The bytes after the last whole record are each of the raw byte tree.

## Appendix F: `vag`

A group is 16-byte SPU ADPCM blocks, each its parameter, its flags, then 28
samples of 4 bits, low nibble first, and `VAGp` headers among them.

**The model**, afresh each group: a probability for headers; 256 byte trees
for a block's parameter, by the last; 256 for its flags, by the last; a
byte tree for raw bytes; 192 trees of 4 bits for the samples. One channel
as `xa`'s, whose 3 inputs of the other channel stay 0; a last parameter and
last flags, 0.

From the group's start, `at`, while a block of 16 bytes is left:

1. If 48 bytes are left, a bit of the header probability: 1 is a header,
   48 bytes with the raw byte tree, its first 4 then set to `VAGp`; the
   channel, last parameter and last flags start again, and `at` moves 48.
2. Otherwise, the block: its parameter with the parameter tree of the last,
   its flags with the flags tree of the last, both then the last; `shift`
   the parameter's low 4 bits, taken as 9 above 12, `filter` its high 4,
   taken as 0 above 4. Its 28 samples are coded as `xa`'s, step 3, its
   nibble `j` that of byte `2 + j / 2`, the low one for an even `j`, with
   `K0 = [0, 60, 115, 98, 122]` and `K1 = [0, 0, −52, −55, −60]`, and no
   other channel. `at` moves 16.

The bytes after the last block are each of the raw byte tree.

## Appendix G: `str`

A group is STR video sectors' data, as a CD data track's video plane holds
them (§8.4): 2,048 bytes a Form 1 sector, 2,324 a Form 2 one. A frame's
sectors are gathered, its MDEC bitstream parsed into its codes, and the
codes coded in context of the frame before's.

### G.1 The bitstream

A frame's bitstream, after an 8-byte BS header, is 16-bit little-endian
words, each read high bit first. The BS header's version, its bytes 6-7, is
1 or 2, both read as v2, or 3. A frame of `m` macroblocks holds `6m`
blocks, each macroblock's Cr, Cb, Y1, Y2, Y3 and Y4; a block is a DC, then
AC codes, then the end of block, `10`:

- **v2's DC** is 10 bits, two's complement.
- **v3's DC** is MPEG-1's: a size class `k`, by its code, chroma's `00 01
  10 110 1110 11110 111110 1111110 11111110` or luma's `100 00 01 101 110
  1110 11110 111110 1111110` for classes 0 to 8, then `k` bits `v`, the
  difference `v` if its top bit is 1, `v − 2ᵏ + 1` if 0, 0 for class 0.
  The DC is the sum of the frame's differences so far of its kind, Cr's,
  Cb's or the four lumas', and 4 times it lies within −512 to 511.
- **An AC code** is MPEG-1 video's `dct_coeff_next` code of its run and
  level (ISO/IEC 11172-2 Annex B; MPEG-2's Table B.14), then a sign bit, 1
  negative; or the escape, `000001`, its run in 6 bits and its
  level in 10, two's complement. A block's codes reach no further than its
  64th coefficient: each moves `run + 1` from the DC.

A code is *escaped* when it is written as the escape though the table has
its run and level. Writing codes back: the BS header's 8 bytes; each code
as above, a v3 DC as its difference from its kind's last, of the class of
that difference's bit length, at most 8; then the frame's *tail*, the bits
after the codes to the frame's end; and the bits, high bit first, made
16-bit little-endian words again.

### G.2 The model

Afresh each group: a tree of 3 bits for what comes next; a 64-probability
number for a frame's sectors; *fields* for the sectors' headers of each
frame kind, 32, 8 and 32 bytes, and for the BS header, 8; a byte tree for
raw bytes; probabilities for the camera, whether a frame is parsed, its
padding, and its tail's zeros; a byte tree for the padding; 11 tail
probabilities; and for the codes, 4 *same* probabilities, 24 64-probability
numbers and 24 sign probabilities for DCs, 4,096 end-of-block probabilities,
128 numbers for runs, 2,048 numbers for levels, and a sign and an escape
probability.

*Fields* of `n` bytes code `n` bytes in place: for each position `i`, a bit
of its `same` probability, 1 when the byte is the last coded at `i`; 0, then
the byte with its byte tree. The last at each position starts 0.

`bucket(v)` is 0 for 0, 1 for 1, 2 for 2 and 3, and 3 above.

### G.3 A group

`last`, the frame before, starts with no blocks. From the group's start,
while 2,048 bytes are left, a value of the 3-bit tree says what comes next:

- **1, a standard frame**, of Form 1 sectors, 32-byte headers; **2, a Final
  Fantasy VIII frame**, `SMJ\1`, of Form 1 sectors, 8-byte headers; **3, a
  Final Fantasy IX frame**, of Form 2 sectors, 32-byte headers;
- **4, a Form 2 sector of no frame**, 2,324 bytes, and **any other value, a
  Form 1 sector of no frame**, 2,048: each byte with the raw byte tree.

A frame: its sectors' count, a number above 0, the sectors within the
group; each sector's header, in
order, with its kind's fields. A Final Fantasy IX frame's sectors are then
taken last first. The first sector taken gives the frame's size: for a
standard frame, `used`, its header's 32-bit little-endian word at 12, and
`m = ⌈w / 16⌉ × ⌈h / 16⌉`, `w` and `h` its 16-bit words at 16 and 18; a
Final Fantasy IX frame's `used` is 4 times its word at 12, its `m` as a
standard frame's; a Final Fantasy VIII frame's `used` is all of it, and its
`m` 280. The frame's payload is its sectors' bytes after their headers, in
the order taken; its first `used` bytes, at most all of them, are coded as
G.4 says, then the rest, its padding: a bit of the padding probability, 1
when it is one byte over and over, that byte with the padding tree, 0 when
there is none; 0, then each byte with the raw byte tree.

The bytes after the last item are each of the raw byte tree.

### G.4 A frame's bytes

1. A bit of the camera probability: 1 when the first 40 bytes are Final
   Fantasy VII's camera data, each then with the raw byte tree.
2. A bit of the parsed probability: 0, then the rest, the bitstream, each
   byte with the raw byte tree, and `last` stays as it was; 1, it is coded
   as its codes:
3. Its BS header, with the BS header's fields. The bitstream is at least 8
   bytes, of an even length, at least `3m` bytes, `m` above 0, its version
   known, or the group is corrupt.
4. Its `6m` blocks' codes (G.5), against `last`.
5. Its tail, of as many bits as the codes written back leave: its first 10
   bits, each with its tail probability `i`; then, if more are left, a bit
   of the tail's zeros probability: 1, they are zeros; 0, each with tail
   probability 10.
6. The codes and the tail, written back as G.1 says, are the bitstream;
   `last` is now this frame.

### G.5 A frame's blocks

`comparable` is whether `last` has as many blocks. `dc[3]`, the last DC of
each kind, Cr, Cb and luma, `class[3]` and `left` start 0. For each block
`b`: `luma` is 1 for Y1 to Y4, `kind` its DC's, `before` block `b` of
`last`, none if it has no such block.

1. If `comparable`, a bit of `same[luma × 2 + left]`, which `left` then is:
   1, the block is `before`, and `dc[kind]` its DC.
2. Otherwise, from `before`: `size = bucket(max(codes − 2, 0))`, its codes
   counted with its DC and end; `there[i]`, the magnitude of its AC level at
   position `i`, 0 to 63, positions counted from the DC, `run + 1` each, past
   63 at 63; and `later[i]`, whether `there` has a non-zero level at `i` or
   past it.
3. The DC: `ctx = kind × 8 + class[kind]`; a magnitude, the DC number
   `ctx`; for one above 0, a bit of the DC sign `ctx`, 1 negative; the DC
   is `dc[kind]` plus or minus it, which `dc[kind]` then is;
   `class[kind]` is the magnitude's bit length, at most 7.
4. Its codes, `position`, `coded` and `previous` starting 0:
   - The end, at position 63 or past without a bit; otherwise a bit of
     end-of-block `((((luma × 64 + position) × 4 + bucket(coded)) × 4 +
     size) × 2 + later[position + 1])`, 1 the end.
   - The run, the run number `(luma × 16 + min(position, 15)) × 4 +
     bucket(previous)`, at most `62 − position`; `position` moves `run + 1`.
   - The level's magnitude, the level number `((luma × 64 + position) × 4 +
     bucket(previous)) × 4 + bucket(there[position])`, at most 512; for one
     above 0, a bit of the sign probability, 1 negative.
   - Whether it is escaped: a bit of the escape probability when the table
     has its run and magnitude, escaped without a bit when it has not.
   - `coded` counts it, and `previous` is its magnitude.
