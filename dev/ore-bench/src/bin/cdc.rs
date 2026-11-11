//! `cdc compare <DISC>...` prints, tab-separated, every chunking algorithm
//! over the discs, deduped across them all: chunks, records, what is stored
//! and what it compresses to in 1 MiB Zstandard groups. `cdc order
//! <DISC>...` prints what each order of stored chunks weighs in 1 MiB and
//! 4 MiB Zstandard groups. Sizes in bytes. See `ore_bench::cdc`.

use std::path::Path;

use anyhow::{Result, bail};
use ore_bench::cdc::{Chunker, compare, order};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let discs: Vec<&Path> = args.iter().skip(1).map(Path::new).collect();
    match args.first().map(String::as_str) {
        Some("compare") => {
            let k = |n: usize| n << 10;
            let mut chunkers: Vec<Chunker> =
                [8, 12, 16, 24, 32].map(|n| Chunker::FastCdc(k(n))).to_vec();
            chunkers.extend([
                Chunker::MinCdc(k(16)),
                Chunker::MothCdc(k(16)),
                Chunker::SeqCdc(k(16)),
                Chunker::UltraCdc(k(16)),
                Chunker::SuperCdc(k(16)),
                Chunker::LeapCdc(k(16)),
                Chunker::Rabin(k(16)),
                Chunker::QuickCdc(k(16)),
            ]);
            let results = compare(&discs, &chunkers)?;
            println!(
                "chunker\tbytes\tchunks\taverage\trecords\tstored_chunks\tstored_bytes\tcompressed\tchunk_mb_per_s"
            );
            for (chunker, result) in chunkers.iter().zip(results) {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.0}",
                    chunker.name(),
                    result.bytes,
                    result.chunks,
                    result.bytes / result.chunks.max(1),
                    result.records,
                    result.stored_chunks,
                    result.stored_bytes,
                    result.compressed,
                    result.bytes as f64 / 1e6 / result.chunk_seconds
                );
            }
        }
        Some("order") => {
            println!("order\tzstd19_1M\tzstd19_4M");
            for (order, one, four) in order(&discs)? {
                println!("{order}\t{one}\t{four}");
            }
        }
        _ => bail!("usage: cdc compare <DISC>... | cdc order <DISC>..."),
    }
    Ok(())
}
