//! Import Retool clone lists, which tell `INFER_CLONES` which titles are the
//! same game under another name, such as Wipeout XL and Wipeout 2097, and which
//! of a region's releases to prefer.

use super::database::*;
use super::model::*;
use super::progress::*;
use anyhow::{Context, Result};
use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use indicatif::ProgressBar;
use regex::Regex;
use sqlx::sqlite::SqliteConnection;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

/// The release groups a clone list name ends with, as Retool names them
const RELEASE_GROUPS: &[&str] = &["MAME Redump", "No-Intro", "Redump", "TOSEC"];

pub fn subcommand() -> Command {
    Command::new("import-clone-lists")
        .about("Import Retool clone lists into oxyromon")
        .arg(
            Arg::new("CLONE_LISTS")
                .help("Set the clone list files to import")
                .required(true)
                .num_args(1..)
                .index(1)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("FORCE")
                .short('f')
                .long("force")
                .help("Force import of clone lists already imported")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> Result<()> {
    let force = matches.get_flag("FORCE");
    for path in matches.get_many::<PathBuf>("CLONE_LISTS").unwrap() {
        import_clone_list(connection, progress_bar, path, force).await?;
    }
    Ok(())
}

/// Import one clone list for the system it names, replacing the system's
/// previous one. Returns the number of titles imported, or None when the list
/// was skipped.
async fn import_clone_list(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    path: &Path,
    force: bool,
) -> Result<Option<usize>> {
    print_header(progress_bar, &format!("Importing \"{}\"", path.display()));
    let file =
        File::open(path).with_context(|| format!("Failed to open \"{}\"", path.display()))?;
    let clone_list: CloneListJson = serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("Failed to parse \"{}\"", path.display()))?;
    let name = &clone_list.description.name;
    let version = &clone_list.description.last_updated;

    let (system_name, release_group) = split_release_group(name);
    let Some(system) = find_system_by_name(connection, system_name).await else {
        print_skip(
            progress_bar,
            &format!("No system is named \"{system_name}\", skipping"),
        );
        return Ok(None);
    };
    if let (Some(release_group), Some(system_release_group)) = (
        release_group,
        system.url.as_deref().and_then(get_release_group),
    ) && release_group != system_release_group
    {
        print_skip(
            progress_bar,
            &format!(
                "\"{}\" is a {release_group} clone list, but the system's DAT is from {system_release_group}, skipping",
                name
            ),
        );
        return Ok(None);
    }
    if !force
        && find_clone_list_version_by_system_id(connection, system.id)
            .await
            .as_ref()
            == Some(version)
    {
        print_skip(
            progress_bar,
            &format!("\"{name}\" ({version}) is already imported, skipping"),
        );
        return Ok(None);
    }

    let mut transaction = begin_transaction(connection).await;
    delete_clone_list_by_system_id(&mut transaction, system.id).await;
    let clone_list_id = create_clone_list(&mut transaction, name, version, system.id).await;
    let mut title_count = 0;
    let mut skipped_count = 0;
    for variant in &clone_list.variants {
        // a compilation holds several games, which needs more than a group
        skipped_count += variant.compilations.len();
        for title in variant.titles.iter().chain(variant.supersets.iter()) {
            let name_type = match title.name_type.as_deref() {
                None | Some("short") => CloneListNameType::Short,
                Some("full") => CloneListNameType::Full,
                Some("regex") if Regex::new(&title.search_term).is_ok() => CloneListNameType::Regex,
                _ => {
                    skipped_count += 1;
                    continue;
                }
            };
            let priority = title.priority.unwrap_or(1);
            // the filters come first, so that one whose conditions a game
            // meets takes precedence over the title
            for filter in &title.filters {
                if filter.conditions.region_order.is_some()
                    || (filter.results.group.is_none() && filter.results.priority.is_none())
                    || filter
                        .conditions
                        .match_string
                        .as_ref()
                        .is_some_and(|pattern| Regex::new(pattern).is_err())
                {
                    skipped_count += 1;
                    continue;
                }
                create_clone_list_title(
                    &mut transaction,
                    filter.results.group.as_deref().unwrap_or(&variant.group),
                    &title.search_term,
                    name_type,
                    filter.results.priority.unwrap_or(priority),
                    filter
                        .conditions
                        .match_regions
                        .as_ref()
                        .map(|regions| regions.join(","))
                        .as_deref(),
                    filter
                        .conditions
                        .match_languages
                        .as_ref()
                        .map(|languages| languages.join(","))
                        .as_deref(),
                    filter.conditions.match_string.as_deref(),
                    clone_list_id,
                )
                .await;
            }
            create_clone_list_title(
                &mut transaction,
                &variant.group,
                &title.search_term,
                name_type,
                priority,
                None,
                None,
                None,
                clone_list_id,
            )
            .await;
            title_count += 1;
        }
    }
    commit_transaction(transaction).await;

    print_success(
        progress_bar,
        &format!("Imported {title_count} titles for \"{}\"", system.name),
    );
    if skipped_count > 0 {
        print_info(
            progress_bar,
            &format!(
                "Skipped {skipped_count} compilations, filters and titles that need more than a group"
            ),
        );
    }
    Ok(Some(title_count))
}

/// Split a clone list name such as `Sony - PlayStation (Redump)` into the
/// system name and the release group, when it ends with one
fn split_release_group(name: &str) -> (&str, Option<&str>) {
    RELEASE_GROUPS
        .iter()
        .find_map(|release_group| {
            name.strip_suffix(&format!(" ({release_group})"))
                .map(|system_name| (system_name, Some(*release_group)))
        })
        .unwrap_or((name, None))
}

/// The release group a DAT's URL belongs to, as Retool tells them apart
fn get_release_group(url: &str) -> Option<&'static str> {
    let url = url.to_lowercase();
    if url.contains("no-intro") {
        Some("No-Intro")
    } else if url.contains("redump") {
        Some("Redump")
    } else if url.contains("tosecdev") {
        Some("TOSEC")
    } else {
        None
    }
}

#[cfg(test)]
mod test_import_clone_lists;
#[cfg(test)]
mod test_import_clone_lists_release_group;
#[cfg(test)]
mod test_import_clone_lists_unknown_system;
#[cfg(test)]
mod test_split_release_group;
