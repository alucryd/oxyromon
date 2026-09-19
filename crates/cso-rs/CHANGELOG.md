# Changelog

## 0.1.0

Initial release: a Rust port of maxcso's CSO/ZSO core.

- CSO v1 and ZSO, compress and decompress, both directions.
- Parallel block compression and decompression over rayon, with positional
  reads and a single ordered writer.
- maxcso's per-block cost model, including the LZ4-versus-DEFLATE tie-break
  and the rule that reverts a block to stored when alignment padding would
  swallow the savings.
- `index_shift` handling for images past 2 GiB, with a bounded LZ4 block
  decoder that tolerates the padding between blocks.
- Compressors: zlib (all four strategies), libdeflate, Zopfli, LZ4 and
  LZ4 HC.
- `csors` CLI behind the default `cli` feature.
- Verified byte-for-byte against the reference maxcso binary in both
  directions, at every block size, up to a 4.5 GiB image with
  `index_shift = 2`.
