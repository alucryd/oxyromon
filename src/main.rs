extern crate clap;
extern crate crc;
#[macro_use]
extern crate diesel;
extern crate dotenv;
extern crate quick_xml;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate simple_error;
extern crate rayon;
extern crate uuid;

mod chdman;
mod checksum;
mod convert_roms;
mod crud;
mod import_dats;
mod import_roms;
mod maxcso;
mod model;
mod prompt;
mod purge_roms;
mod schema;
mod sevenzip;
mod sort_roms;

use self::convert_roms::convert_roms;
use self::import_dats::import_dats;
use self::import_roms::import_roms;
use self::purge_roms::purge_roms;
use self::sort_roms::sort_roms;
use clap::{App, Arg, SubCommand};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use dotenv::dotenv;
use std::env;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let import_dats_subcommand: App = SubCommand::with_name("import-dats")
        .about("Parses and imports No-Intro and Redump DAT files into oxyromon")
        .arg(
            Arg::with_name("DATS")
                .help("Sets the DAT files to import")
                .required(true)
                .multiple(true)
                .index(1),
        )
        .arg(
            Arg::with_name("INFO")
                .short("i")
                .long("info")
                .help("Show the DAT information and exit")
                .required(false),
        );

    let import_roms_subcommand: App = SubCommand::with_name("import-roms")
        .about("Validates and imports ROM files into oxyromon")
        .arg(
            Arg::with_name("ROMS")
                .help("Sets the ROM files to import")
                .required(true)
                .multiple(true)
                .index(1),
        );

    let sort_roms_subcommand: App = SubCommand::with_name("sort-roms")
        .about("Sorts ROM files according to region and version preferences")
        .arg(
            Arg::with_name("REGIONS")
                .short("r")
                .long("regions")
                .help("Sets the regions to keep (unordered)")
                .required(false)
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("1G1R")
                .short("g")
                .long("1g1r")
                .help("Sets the 1G1R regions to keep (ordered)")
                .required(false)
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("NO-BETA")
                .long("no-beta")
                .help("Discards beta games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-DEBUG")
                .long("no-debug")
                .help("Discards debug games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-DEMO")
                .long("no-demo")
                .help("Discards demo games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-PROGRAM")
                .long("no-program")
                .help("Discards program games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-PROTO")
                .long("no-proto")
                .help("Discards prototype games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-SAMPLE")
                .long("no-sample")
                .help("Discards sample games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-SEGA-CHANNEL")
                .long("no-sega-channel")
                .help("Discards sega channel games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-VIRTUAL-CONSOLE")
                .long("no-virtual-console")
                .help("Discards virtual console games")
                .required(false),
        )
        .arg(
            Arg::with_name("MISSING")
                .short("m")
                .long("missing")
                .help("Shows missing games")
                .required(false),
        )
        .arg(
            Arg::with_name("ALL")
                .short("a")
                .long("all")
                .help("Sorts all systems")
                .required(false),
        )
        .arg(
            Arg::with_name("YES")
                .short("y")
                .long("yes")
                .help("Automatically says yes to prompts")
                .required(false),
        );

    let convert_roms_subcommand: App = SubCommand::with_name("convert-roms")
        .about("Converts ROM files between common formats")
        .arg(
            Arg::with_name("FORMAT")
                .short("f")
                .long("format")
                .help("Sets the destination format")
                .required(false)
                .takes_value(true)
                .possible_values(&["7Z", "CHD", "ORIGINAL", "ZIP"]),
        );

    let purge_roms_subcommand: App = SubCommand::with_name("purge-roms")
        .about("Purges trashed and missing ROM files")
        .arg(
            Arg::with_name("EMPTY-TRASH")
                .short("t")
                .long("empty-trash")
                .help("Empties the ROM files trash directories")
                .required(false),
        )
        .arg(
            Arg::with_name("YES")
                .short("y")
                .long("yes")
                .help("Automatically says yes to prompts")
                .required(false),
        );

    let matches = App::new("oxyromon")
        .version("0.1.0")
        .about("Rusty ROM OrgaNizer")
        .author("Maxime Gauduin <alucryd@archlinux.org>")
        .subcommands(vec![
            import_dats_subcommand,
            import_roms_subcommand,
            sort_roms_subcommand,
            convert_roms_subcommand,
            purge_roms_subcommand,
        ])
        .get_matches();

    if matches.subcommand.is_some() {
        let connection = establish_connection();
        match matches.subcommand_name() {
            Some("convert-roms") => convert_roms(
                &connection,
                &matches.subcommand_matches("convert-roms").unwrap(),
            )?,
            Some("import-dats") => import_dats(
                &connection,
                &matches.subcommand_matches("import-dats").unwrap(),
            )?,
            Some("import-roms") => import_roms(
                &connection,
                &matches.subcommand_matches("import-roms").unwrap(),
            )?,
            Some("purge-roms") => purge_roms(
                &connection,
                &matches.subcommand_matches("purge-roms").unwrap(),
            )?,
            Some("sort-roms") => sort_roms(
                &connection,
                &matches.subcommand_matches("sort-roms").unwrap(),
            )?,
            _ => (),
        }
    }

    Ok(())
}

fn establish_connection() -> PgConnection {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgConnection::establish(&database_url).expect(&format!("Error connecting to {}", database_url))
}
