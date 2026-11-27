//! PlayStation 3 discs, which circulate as JB folders: the disc's files,
//! decrypted, which the IRD lists. With the IRD kept when it was imported, a
//! JB folder rebuilds the Redump ISO byte for byte, and the ISO extracts back
//! to its JB folder, both by the ps3-rs crate.
//!
//! A JB folder's system update, `PS3_UPDATE/PS3UPDAT.PUP`, is the same file
//! on every disc built on that firmware, so it is kept once a system, in
//! `PS3_UPDATE/<version>/` in its directory, for every game that lists it.

use super::common::*;
use super::config::HashAlgorithm;
use super::database::*;
use super::import_keys::{PS3_KEYS_FILE, load_ps3_keys};
use super::import_roms::create_or_update_romfile;
use super::mimetype::ISO_EXTENSION;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use ps3_rs::{Ird, UPDATE_PATH};
use sqlx::sqlite::SqliteConnection;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Put the system update at `path`, of this size and MD5, where it goes, for
/// every JB folder of `system` listing it without a file yet; if it is kept
/// already, link them to that file and delete this one. Returns their games.
pub async fn place_update(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    path: &Path,
    size: u64,
    md5: &str,
) -> Result<HashSet<i64>> {
    let roms = find_child_roms_without_romfile_by_name_and_size_and_md5(
        connection,
        UPDATE_PATH,
        size,
        md5,
        Some(system.id),
    )
    .await;
    let game_ids: HashSet<i64> = roms.iter().map(|rom| rom.game_id).collect();
    let romfile = CommonRomfile::from_path(&path)?;
    match find_child_romfile_id_by_name_and_size_and_md5(
        connection,
        UPDATE_PATH,
        size as i64,
        md5,
        Some(system.id),
    )
    .await
    {
        Some(romfile_id) => {
            for rom in &roms {
                update_rom_romfile(connection, rom.id, Some(romfile_id)).await;
            }
            romfile.delete(progress_bar, false).await?;
        }
        None => {
            let version = ps3_rs::pup_version(path)
                .with_context(|| format!("Failed to read \"{}\"", path.display()))?;
            let new_path = ps3_update_path(connection, system, &version).await?;
            if new_path != path && new_path.exists() {
                bail!(
                    "\"{}\" exists already, another update of firmware {version}",
                    new_path.display()
                );
            }
            romfile.rename(progress_bar, &new_path, false).await?;
            let roms: Vec<&Rom> = roms.iter().collect();
            create_or_update_romfile(connection, progress_bar, &new_path, &roms, Some(system.id))
                .await?;
        }
    }
    Ok(game_ids)
}

/// Import a loose PS3UPDAT.PUP as the system update of every JB folder that
/// lists it: none when none do, which leaves it to be matched as any file.
pub async fn import_pup(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &Option<&System>,
    romfile: &CommonRomfile,
) -> Result<Option<(HashSet<i64>, HashSet<i64>)>> {
    let (md5, size) = romfile
        .get_hash_and_size(connection, progress_bar, 1, 1, &HashAlgorithm::Md5)
        .await?;
    let roms = find_child_roms_without_romfile_by_name_and_size_and_md5(
        connection,
        UPDATE_PATH,
        size,
        &md5,
        system.map(|system| system.id),
    )
    .await;
    let Some(rom) = roms.first() else {
        if find_child_romfile_id_by_name_and_size_and_md5(
            connection,
            UPDATE_PATH,
            size as i64,
            &md5,
            system.map(|system| system.id),
        )
        .await
        .is_some()
        {
            print_skip(progress_bar, "Already imported");
            return Ok(Some((HashSet::new(), HashSet::new())));
        }
        return Ok(None);
    };
    let game = find_game_by_id(connection, rom.game_id).await;
    let system = find_system_by_id(connection, game.system_id).await;
    let game_ids =
        place_update(connection, progress_bar, &system, &romfile.path, size, &md5).await?;
    print_success(
        progress_bar,
        &format!(
            "Matches the system update of {} JB folder{}",
            game_ids.len(),
            if game_ids.len() == 1 { "" } else { "s" }
        ),
    );
    Ok(Some((HashSet::from([system.id]), game_ids)))
}

/// The keys a game's PS3 disc may be opened with, which ore-rs tells apart
/// by what the disc decrypts to: its IRD's, when one is kept, then the disc
/// keys `import-keys` keeps, which tests leave out.
pub async fn disc_keys(connection: &mut SqliteConnection, game_id: i64) -> Result<Vec<[u8; 16]>> {
    let mut keys = Vec::new();
    if let Some(data) = find_ird_by_game_id(connection, game_id).await {
        keys.push(Ird::parse(&data)?.disc_key());
    }
    let path = super::config::get_data_directory()?.join(PS3_KEYS_FILE);
    if !cfg!(test) && path.is_file() {
        keys.extend(load_ps3_keys(&path)?);
    }
    Ok(keys)
}

/// A JB folder game's IRD, kept since it was imported.
async fn kept_ird(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    game: &Game,
) -> Result<Option<Ird>> {
    match find_ird_by_game_id(connection, game.id).await {
        Some(data) => Ok(Some(Ird::parse(&data).with_context(|| {
            format!("Failed to read the IRD of \"{}\"", game.name)
        })?)),
        None => {
            print_warning(
                progress_bar,
                "No IRD kept for this game, import it again with import-irds",
            );
            Ok(None)
        }
    }
}

/// The ISO ROM a JB folder game's files are the children of.
async fn iso_rom(connection: &mut SqliteConnection, game: &Game) -> Option<Rom> {
    find_roms_by_game_id_no_parents(connection, game.id)
        .await
        .into_iter()
        .next()
}

/// Delete each file no ROM is in any more, and the folders it leaves empty up
/// to the game's own.
async fn delete_unused_romfiles(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile_ids: HashSet<i64>,
    game: &Game,
) -> Result<()> {
    let mut directories: Vec<PathBuf> = Vec::new();
    for romfile_id in romfile_ids {
        // a system update other games' JB folders share
        if !find_roms_by_romfile_id(connection, romfile_id)
            .await
            .is_empty()
        {
            continue;
        }
        let romfile = find_romfile_by_id(connection, romfile_id).await;
        let common = romfile.as_common(connection).await?;
        if common.path.exists() {
            common.delete(progress_bar, true).await?;
        }
        delete_romfile_by_id(connection, romfile_id).await;
        if let Some(parent) = common.path.parent() {
            directories.push(parent.to_path_buf());
        }
    }
    // the deepest first, so that a folder emptied by its children goes too
    directories.sort_by_key(|directory| std::cmp::Reverse(directory.components().count()));
    directories.dedup();
    for directory in directories {
        for directory in directory.ancestors() {
            let empty = directory
                .read_dir()
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
            if !empty {
                break;
            }
            remove_directory(progress_bar, &directory, true).await?;
            if directory.file_name() == Some(game.name.as_ref()) {
                break;
            }
        }
    }
    Ok(())
}

/// Rebuild each JB folder game's ISO from its files and its IRD, checked
/// against the DAT, then delete the files the ISO replaces.
pub async fn jbfolders_to_isos(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    game_ids: &[i64],
) -> Result<()> {
    for &game_id in game_ids {
        let game = find_game_by_id(connection, game_id).await;
        let Some(iso_rom) = iso_rom(connection, &game).await else {
            continue;
        };
        if iso_rom.romfile_id.is_some() {
            continue;
        }
        let children = find_roms_by_parent_id(connection, iso_rom.id).await;
        if children.iter().all(|rom| rom.romfile_id.is_none()) {
            continue;
        }
        print_subheader(progress_bar, &format!("Rebuilding \"{}\"", iso_rom.name));
        if let Some(missing) = children.iter().find(|rom| rom.romfile_id.is_none()) {
            print_warning(
                progress_bar,
                &format!("Missing \"{}\", skipping", missing.name),
            );
            continue;
        }
        let Some(ird) = kept_ird(connection, progress_bar, &game).await? else {
            continue;
        };

        let mut sources: HashMap<String, PathBuf> = HashMap::new();
        for rom in &children {
            let romfile = find_romfile_by_id(connection, rom.romfile_id.unwrap()).await;
            sources.insert(rom.name.clone(), romfile.as_common(connection).await?.path);
        }
        let size = children.iter().map(|rom| rom.size as u64).sum();
        // written where it goes, as ps3-rs writes a .part first: a disc is too
        // large to copy over from another file system
        let output = CommonRomfile::from_path(&PathBuf::from(&iso_rom.name))?
            .get_sorted_path(connection, system, &game, &iso_rom, &None, &None)
            .await?;
        if output.exists() {
            print_warning(
                progress_bar,
                &format!("\"{}\" exists already, skipping", output.display()),
            );
            continue;
        }
        std::fs::create_dir_all(output.parent().unwrap())?;
        let iso_path = output.clone();
        let rebuilt = run_blocking(progress_bar, size, move |progress| {
            ps3_rs::rebuild(
                &ird,
                &mut |path| sources.get(path).cloned(),
                &iso_path,
                progress,
            )
        })
        .await;
        let md5 = match rebuilt {
            Ok(md5) => to_hex(&md5),
            Err(error) => {
                print_error(progress_bar, &format!("{error:#}"));
                continue;
            }
        };
        if Some(&md5) != iso_rom.md5.as_ref() {
            print_error(
                progress_bar,
                &format!(
                    "The rebuilt ISO has MD5 {md5}, expected {}, keeping the JB folder",
                    iso_rom.md5.as_deref().unwrap_or("none")
                ),
            );
            CommonRomfile::from_path(&output)?
                .delete(progress_bar, true)
                .await?;
            continue;
        }

        let mut transaction = begin_transaction(connection).await;
        create_or_update_romfile(
            &mut transaction,
            progress_bar,
            &output,
            &[&iso_rom],
            Some(system.id),
        )
        .await?;
        let mut romfile_ids = HashSet::new();
        for rom in &children {
            romfile_ids.insert(rom.romfile_id.unwrap());
            update_rom_romfile(&mut transaction, rom.id, None).await;
        }
        delete_unused_romfiles(&mut transaction, progress_bar, romfile_ids, &game).await?;
        commit_transaction(transaction).await;
        print_success(progress_bar, "Matches the DAT");
    }
    Ok(())
}

/// Extract each JB folder game's ISO to its JB folder, with the key its IRD
/// gives, every file checked against the IRD, then delete the ISO.
pub async fn isos_to_jbfolders(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    game_ids: &[i64],
) -> Result<()> {
    for &game_id in game_ids {
        let game = find_game_by_id(connection, game_id).await;
        let Some(iso_rom) = iso_rom(connection, &game).await else {
            continue;
        };
        let Some(iso_romfile_id) = iso_rom.romfile_id else {
            continue;
        };
        let iso_romfile = find_romfile_by_id(connection, iso_romfile_id).await;
        let iso = iso_romfile.as_common(connection).await?;
        print_subheader(progress_bar, &format!("Extracting \"{}\"", iso_rom.name));
        if !iso
            .path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(ISO_EXTENSION))
        {
            print_warning(
                progress_bar,
                "Not a plain ISO, convert it to ORIGINAL first, skipping",
            );
            continue;
        }
        let Some(ird) = kept_ird(connection, progress_bar, &game).await? else {
            continue;
        };

        // extracted where it goes, its root where the disc's root file sorts:
        // a disc is too large to copy over from another file system
        let Some(sfb) = find_rom_by_name_and_game_id(connection, "PS3_DISC.SFB", game.id).await
        else {
            print_warning(progress_bar, "The IRD lists no PS3_DISC.SFB, skipping");
            continue;
        };
        let output = CommonRomfile::from_path(&PathBuf::from(&sfb.name))?
            .get_sorted_path(connection, system, &game, &sfb, &None, &None)
            .await?
            .parent()
            .unwrap()
            .to_path_buf();
        if output.exists() {
            print_warning(
                progress_bar,
                &format!("\"{}\" exists already, skipping", output.display()),
            );
            continue;
        }
        let key = ird.disc_key();
        let (iso_path, jb_path) = (iso.path.clone(), output.clone());
        let size = std::fs::metadata(&iso.path)?.len();
        let extracted = match run_blocking(progress_bar, size, move |progress| {
            ps3_rs::extract(&iso_path, &key, &jb_path, progress)
        })
        .await
        {
            Ok(extracted) => extracted,
            Err(error) => {
                print_error(progress_bar, &format!("{error:#}"));
                continue;
            }
        };

        let children: HashMap<String, Rom> = find_roms_by_parent_id(connection, iso_rom.id)
            .await
            .into_iter()
            .map(|rom| (rom.name.clone(), rom))
            .collect();
        let wrong = extracted.iter().find(|(file, md5)| {
            children.get(&file.path).is_none_or(|rom| {
                rom.size as u64 != file.size() || rom.md5.as_deref() != Some(&to_hex(md5))
            })
        });
        let mismatch = match wrong {
            Some((file, _)) => Some(format!(
                "\"{}\" does not match the IRD, keeping the ISO",
                file.path
            )),
            None if extracted.len() != children.len() => Some(format!(
                "{} files extracted, the IRD lists {}, keeping the ISO",
                extracted.len(),
                children.len()
            )),
            None => None,
        };
        if let Some(mismatch) = mismatch {
            print_error(progress_bar, &mismatch);
            remove_directory(progress_bar, &output, true).await?;
            continue;
        }

        let mut transaction = begin_transaction(connection).await;
        for (file, md5) in &extracted {
            let rom = &children[&file.path];
            let path = output.join(&file.path);
            if file.path == UPDATE_PATH {
                place_update(
                    &mut transaction,
                    progress_bar,
                    system,
                    &path,
                    file.size(),
                    &to_hex(md5),
                )
                .await?;
                continue;
            }
            let romfile = CommonRomfile::from_path(&path)?;
            let new_path = romfile
                .get_sorted_path(&mut transaction, system, &game, rom, &None, &None)
                .await?;
            romfile.rename(progress_bar, &new_path, true).await?;
            create_or_update_romfile(
                &mut transaction,
                progress_bar,
                &new_path,
                &[rom],
                Some(system.id),
            )
            .await?;
        }
        // the folder the update was extracted to, which it left
        let update_directory = output.join(Path::new(UPDATE_PATH).parent().unwrap());
        if update_directory
            .read_dir()
            .is_ok_and(|mut entries| entries.next().is_none())
        {
            remove_directory(progress_bar, &update_directory, true).await?;
        }
        update_rom_romfile(&mut transaction, iso_rom.id, None).await;
        iso.delete(progress_bar, false).await?;
        delete_romfile_by_id(&mut transaction, iso_romfile_id).await;
        commit_transaction(transaction).await;
        print_success(
            progress_bar,
            &format!("Extracted {} files, matching the IRD", extracted.len()),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
