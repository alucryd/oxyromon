use super::database::*;
use clap::{App, Arg, ArgMatches, SubCommand};
use simple_error::SimpleResult;
use sqlx::sqlite::{Sqlite, SqlitePool};
use tide_sqlx::SQLxMiddleware;
use tide_sqlx::SQLxRequestExt;

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("server")
        .about("Launches the backend server")
        .arg(
            Arg::with_name("address")
                .short("a")
                .long("address")
                .help("Specifies the server address")
                .required(false)
                .takes_value(true)
                .default_value("127.0.0.1"),
        )
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .help("Specifies the server port")
                .required(false)
                .takes_value(true)
                .default_value("8080"),
        )
}

pub async fn main(pool: &SqlitePool, matches: &ArgMatches<'_>) -> SimpleResult<()> {
    let mut app = tide::new();
    app.with(SQLxMiddleware::from(pg_pool));
    app.at("/api/systems").get(get_systems);
    try_with!(app.listen("127.0.0.1:8080").await, "Failed to run server");
    Ok(())
}

async fn get_systems(req: tide::Request<()>) {
    let mut connection = req.sqlx_conn::<Sqlite>().await;
}
