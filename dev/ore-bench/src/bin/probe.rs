//! `probe wii <DISC>` prints a Wii disc's hash exceptions and what lies
//! outside its partitions; `probe speed <DISC>` how fast groups of its
//! deduped chunks compress and decompress on one core. Tab-separated, with a
//! header line. See `ore_bench::probe`.

use std::path::Path;

use anyhow::{Result, bail};
use ore_bench::probe::{speed, wii};

fn name(disc: &Path) -> String {
    disc.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["wii", disc] => {
            let disc = Path::new(disc);
            let probe = wii(disc)?;
            println!(
                "disc\tpartitions\tblocks\texceptions\tblocks_with_exceptions\toutside\toutside_zero\toutside_padding\toutside_other\toutside_other_zstd19"
            );
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                name(disc),
                probe.partitions,
                probe.blocks,
                probe.exceptions,
                probe.blocks_with_exceptions,
                probe.outside,
                probe.outside_zero,
                probe.outside_padding,
                probe.outside_other,
                probe.outside_other_compressed
            );
        }
        ["speed", disc] => {
            let disc = Path::new(disc);
            let speeds = speed(
                disc,
                256 << 20,
                &[256 << 10, 1 << 20, 4 << 20],
                &["zstd19", "lzma2"],
            )?;
            println!(
                "disc\tcodec\tgroup\tgroups\tinput\toutput\tcompress_seconds\tdecompress_seconds"
            );
            for speed in speeds {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}",
                    name(disc),
                    speed.codec,
                    speed.group,
                    speed.groups,
                    speed.input,
                    speed.output,
                    speed.compress_seconds,
                    speed.decompress_seconds
                );
            }
        }
        _ => bail!("usage: probe wii <DISC> | probe speed <DISC>"),
    }
    Ok(())
}
