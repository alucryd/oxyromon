use super::database::*;
use super::model::*;
use async_ctrlc::CtrlC;
use async_graphql::dataloader::{DataLoader, Loader};
use async_graphql::{
    Context, EmptyMutation, EmptySubscription, FieldError, Object, Result, Schema,
};
use async_std::prelude::FutureExt;
use async_trait::async_trait;
use clap::{App, Arg, ArgMatches, SubCommand};
use lazy_static::lazy_static;
use once_cell::sync::OnceCell;
use simple_error::SimpleResult;
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;

lazy_static! {
    static ref POOL: OnceCell<SqlitePool> = OnceCell::new();
}

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("server")
        .about("Launches the backend server")
        .arg(
            Arg::with_name("ADDRESS")
                .short("a")
                .long("address")
                .help("Specifies the server address")
                .required(false)
                .takes_value(true)
                .default_value("127.0.0.1"),
        )
        .arg(
            Arg::with_name("PORT")
                .short("p")
                .long("port")
                .help("Specifies the server port")
                .required(false)
                .takes_value(true)
                .default_value("8080"),
        )
}

pub struct SystemLoader;

#[async_trait]
impl Loader<i64> for SystemLoader {
    type Value = System;
    type Error = FieldError;

    async fn load(&self, _: &[i64]) -> Result<HashMap<i64, Self::Value>, Self::Error> {
        Ok(
            find_systems(&mut POOL.get().unwrap().acquire().await.unwrap())
                .await
                .into_iter()
                .map(|system| (system.id, system))
                .collect(),
        )
    }
}

pub struct GameLoader;

#[async_trait]
impl Loader<i64> for GameLoader {
    type Value = Game;
    type Error = FieldError;

    async fn load(&self, _: &[i64]) -> Result<HashMap<i64, Self::Value>, Self::Error> {
        Ok(
            find_games(&mut POOL.get().unwrap().acquire().await.unwrap())
                .await
                .into_iter()
                .map(|game| (game.id, game))
                .collect(),
        )
    }
}
struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn systems(&self) -> Result<Vec<System>> {
        Ok(find_systems(&mut POOL.get().unwrap().acquire().await.unwrap()).await)
    }

    async fn system(&self, ctx: &Context<'_>, id: i64) -> Result<Option<System>> {
        Ok(ctx
            .data_unchecked::<DataLoader<SystemLoader>>()
            .load_one(id)
            .await?)
    }

    async fn games(&self) -> Result<Vec<Game>> {
        Ok(find_games(&mut POOL.get().unwrap().acquire().await.unwrap()).await)
    }

    async fn game(&self, ctx: &Context<'_>, id: i64) -> Result<Option<Game>> {
        Ok(ctx
            .data_unchecked::<DataLoader<GameLoader>>()
            .load_one(id)
            .await?)
    }
}

#[derive(Clone)]
struct AppState {
    schema: Schema<QueryRoot, EmptyMutation, EmptySubscription>,
}

pub async fn main(pool: SqlitePool, matches: &ArgMatches<'_>) -> SimpleResult<()> {
    POOL.set(pool)
        .expect("Failed to set database connection pool");
    let ctrlc = CtrlC::new().expect("Cannot use CTRL-C handler");
    let schema = Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .data(DataLoader::new(SystemLoader))
        .finish();
    ctrlc
        .race(async {
            let mut app = tide::new();
            app.at("/graphql")
                .post(async_graphql_tide::endpoint(schema));
            let address = matches.value_of("ADDRESS").unwrap();
            let port = matches.value_of("PORT").unwrap();
            app.listen(format!("{}:{}", address, port))
                .await
                .expect("Failed to run server");
        })
        .await;
    close_connection(&POOL.get().unwrap()).await;
    Ok(())
}
