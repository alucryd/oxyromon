use super::archive::AsArchive;
use super::chd::{AsChd, ChdRomfile, ChdType};
use super::common::*;
use super::config::get_rom_directory;
use super::database::find_romfile_by_id;
use super::mimetype::CHD_EXTENSION;
use super::model::{Rom, Romfile};
use super::nsz::{AsNsz, NspRomfile, ToNsp};
use super::rvz::AsRvz;
use super::xso::AsXso;
use anyhow::Result;
use indexmap::map::IndexMap;
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tempfile::TempDir;

/// A romfile decoded to a neutral format, along with the original file it came from.
pub struct Decoded<T> {
    pub source: CommonRomfile,
    pub inner: T,
}

/// Extracts the single file contained in an archive into the tmp directory.
pub async fn archive_to_common(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    rom: &Rom,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<Decoded<CommonRomfile>> {
    let archive_romfile = romfile
        .as_common(connection)
        .await?
        .as_archive(progress_bar, Some(rom))
        .await?
        .pop()
        .unwrap();
    let inner = archive_romfile
        .to_common(progress_bar, &tmp_directory.path())
        .await?;
    Ok(Decoded {
        source: archive_romfile.romfile,
        inner,
    })
}

/// Views a plain romfile as its own source, no decoding needed.
pub async fn common_as_source(
    connection: &mut SqliteConnection,
    romfile: &Romfile,
) -> Result<Decoded<CommonRomfile>> {
    let common_romfile = romfile.as_common(connection).await?;
    Ok(Decoded {
        source: common_romfile.clone(),
        inner: common_romfile,
    })
}

/// Parses a CHD romfile, resolving its parent if any.
pub async fn romfile_as_chd(
    connection: &mut SqliteConnection,
    romfile: &Romfile,
) -> Result<ChdRomfile> {
    match romfile.parent_id {
        Some(parent_id) => {
            let parent_chd_romfile = find_romfile_by_id(connection, parent_id)
                .await
                .as_common(connection)
                .await?
                .as_chd()
                .await?;
            romfile
                .as_common(connection)
                .await?
                .as_chd_with_parent(parent_chd_romfile)
                .await
        }
        None => romfile.as_common(connection).await?.as_chd().await,
    }
}

/// The lineage-relevant part of a CHD's header.
struct ParsedChd {
    game_id: i64,
    romfile_id: i64,
    path: PathBuf,
    chd_sha1: String,
    parent_sha1: Option<String>,
}

/// How many children levels hang below `game_id`, memoized.
fn depth(
    game_id: i64,
    children_by_game: &HashMap<i64, Vec<i64>>,
    depths: &mut HashMap<i64, usize>,
) -> usize {
    if let Some(depth) = depths.get(&game_id) {
        return *depth;
    }
    // CHDs reading from each other in a cycle cannot be ordered: break the
    // cycle by treating the game as if it had no children
    depths.insert(game_id, 0);
    let mut max_depth = 0;
    for child_game_id in children_by_game
        .get(&game_id)
        .map_or(&[] as &[i64], Vec::as_slice)
    {
        max_depth = max_depth.max(depth(*child_game_id, children_by_game, depths) + 1);
    }
    depths.insert(game_id, max_depth);
    max_depth
}

/// Which CHD of a batch of games reads from which, learned from the CHDs.
///
/// A child CHD stores the SHA-1 of the parent it reads from, but the `parent_id`
/// column only records that link for the CHDs oxyromon created itself, so the
/// children of an imported set are only known through their headers. Converting
/// a CHD rewrites or removes it: children have to be converted before their
/// parents, and a parent whose children are still on disk has to be left alone.
pub struct ChdLineage {
    /// how deep each game sits in its chain, 0 for a CHD no other reads from
    depths: HashMap<i64, usize>,
    /// the paths of the children reading from each CHD, by romfile id
    children: HashMap<i64, Vec<PathBuf>>,
}

impl ChdLineage {
    /// Reads the header of every CHD of `games` to find which of them read from
    /// which.
    pub async fn load(
        connection: &mut SqliteConnection,
        games: &IndexMap<i64, Vec<Rom>>,
        romfiles_by_id: &HashMap<i64, Romfile>,
    ) -> Result<Self> {
        let rom_directory = get_rom_directory(connection).await;
        let mut parsed: Vec<ParsedChd> = Vec::new();
        let mut seen: HashSet<i64> = HashSet::new();
        for (game_id, roms) in games {
            for rom in roms {
                let romfile_id = rom.romfile_id.unwrap();
                // the cue sheet of a CD's CHD is a second rom on the same romfile
                if !seen.insert(romfile_id) {
                    continue;
                }
                let romfile = romfiles_by_id.get(&romfile_id).unwrap();
                if !romfile.path.ends_with(CHD_EXTENSION) {
                    continue;
                }
                let path = rom_directory.join(&romfile.path);
                let common_romfile = CommonRomfile {
                    path: path.clone(),
                    system_id: None,
                };
                // a CHD whose header cannot be read cannot be ordered, and the
                // conversion of that CHD is what will report it
                if let Ok((_, _, _, chd_sha1, parent_sha1, _)) = common_romfile.parse_chd().await {
                    parsed.push(ParsedChd {
                        game_id: *game_id,
                        romfile_id,
                        path,
                        chd_sha1,
                        parent_sha1,
                    });
                }
            }
        }

        let games_by_chd_sha1: HashMap<&str, (i64, i64)> = parsed
            .iter()
            .map(|chd| (chd.chd_sha1.as_str(), (chd.game_id, chd.romfile_id)))
            .collect();
        // the games holding the CHDs read by each game's CHDs, and the children
        // of each CHD
        let mut children_by_game: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut children: HashMap<i64, Vec<PathBuf>> = HashMap::new();
        for chd in &parsed {
            let Some(parent_sha1) = &chd.parent_sha1 else {
                continue;
            };
            let Some((parent_game_id, parent_romfile_id)) =
                games_by_chd_sha1.get(parent_sha1.as_str())
            else {
                // the parent is not part of the conversion, it cannot be ordered either
                continue;
            };
            if *parent_game_id == chd.game_id {
                continue;
            }
            children_by_game
                .entry(*parent_game_id)
                .or_default()
                .push(chd.game_id);
            children
                .entry(*parent_romfile_id)
                .or_default()
                .push(chd.path.clone());
        }

        let mut depths: HashMap<i64, usize> = HashMap::new();
        for game_id in games.keys() {
            depth(*game_id, &children_by_game, &mut depths);
        }
        Ok(Self { depths, children })
    }

    /// Orders `games` so that children are converted before their parents, and
    /// parents whose children are still to come after them.
    pub fn sort(&self, games: &mut IndexMap<i64, Vec<Rom>>) {
        games.sort_by_key(|game_id, _| self.depths.get(game_id).copied().unwrap_or(0));
    }

    /// Whether children of the CHD are still on disk, and were therefore not
    /// converted before it.
    pub fn has_children(&self, romfile_id: i64) -> bool {
        self.children
            .get(&romfile_id)
            .is_some_and(|children| children.iter().any(|path| path.is_file()))
    }
}

/// Decodes a DVD CHD to ISO in the tmp directory, resolving its parent if any.
/// Returns None when the CHD is not a DVD.
pub async fn chd_to_iso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<Option<Decoded<IsoRomfile>>> {
    let chd_romfile = romfile_as_chd(connection, romfile).await?;
    if chd_romfile.chd_type != ChdType::Dvd {
        return Ok(None);
    }
    let inner = chd_romfile
        .to_iso(progress_bar, &tmp_directory.path())
        .await?;
    Ok(Some(Decoded {
        source: chd_romfile.romfile,
        inner,
    }))
}

/// Decodes a CSO/ZSO to ISO in the tmp directory.
pub async fn xso_to_iso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<Decoded<IsoRomfile>> {
    let xso_romfile = romfile.as_common(connection).await?.as_xso().await?;
    let inner = xso_romfile
        .to_iso(progress_bar, &tmp_directory.path())
        .await?;
    Ok(Decoded {
        source: xso_romfile.romfile,
        inner,
    })
}

/// Decodes an NSZ to NSP in the tmp directory.
pub async fn nsz_to_nsp(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<Decoded<NspRomfile>> {
    let nsz_romfile = romfile.as_common(connection).await?.as_nsz()?;
    let inner = nsz_romfile
        .to_nsp(progress_bar, &tmp_directory.path())
        .await?;
    Ok(Decoded {
        source: nsz_romfile.romfile,
        inner,
    })
}

/// Decodes an RVZ to ISO in the tmp directory.
pub async fn rvz_to_iso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<Decoded<IsoRomfile>> {
    let rvz_romfile = romfile.as_common(connection).await?.as_rvz()?;
    let inner = rvz_romfile
        .to_iso(progress_bar, &tmp_directory.path())
        .await?;
    Ok(Decoded {
        source: rvz_romfile.romfile,
        inner,
    })
}

/// Extracts a CUE/BIN set from an archive into the tmp directory.
pub async fn archive_to_cue_bin(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    cue_rom: &Rom,
    bin_roms: &[&Rom],
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<Decoded<CueBinRomfile>> {
    let source = romfile.as_common(connection).await?;
    let cue_romfile = source
        .clone()
        .as_archive(progress_bar, Some(cue_rom))
        .await?
        .pop()
        .unwrap()
        .to_common(progress_bar, &tmp_directory.path())
        .await?;
    let mut bin_romfiles: Vec<CommonRomfile> = vec![];
    for bin_rom in bin_roms {
        bin_romfiles.push(
            source
                .clone()
                .as_archive(progress_bar, Some(bin_rom))
                .await?
                .pop()
                .unwrap()
                .to_common(progress_bar, &tmp_directory.path())
                .await?,
        );
    }
    Ok(Decoded {
        source,
        inner: cue_romfile.as_cue_bin(bin_romfiles)?,
    })
}

/// Assembles a CUE/BIN set from loose cue and bin romfiles.
pub async fn assemble_cue_bin(
    connection: &mut SqliteConnection,
    romfiles_by_id: &HashMap<i64, Romfile>,
    cue_rom: &Rom,
    bin_roms: &[&Rom],
) -> Result<CueBinRomfile> {
    let cue_romfile = romfiles_by_id
        .get(&cue_rom.romfile_id.unwrap())
        .unwrap()
        .as_common(connection)
        .await?;
    let mut bin_romfiles: Vec<CommonRomfile> = vec![];
    for bin_rom in bin_roms {
        bin_romfiles.push(
            romfiles_by_id
                .get(&bin_rom.romfile_id.unwrap())
                .unwrap()
                .as_common(connection)
                .await?,
        );
    }
    cue_romfile.as_cue_bin(bin_romfiles)
}
