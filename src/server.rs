use super::database::*;
use super::model::*;
use async_ctrlc::CtrlC;
use async_graphql::dataloader::{DataLoader, Loader};
use async_graphql::futures_util::TryStreamExt;
use async_graphql::{
    ComplexObject, Context, EmptyMutation, EmptySubscription, FieldError, Object, Result, Schema,
};
use async_std::prelude::FutureExt;
use async_trait::async_trait;
use clap::{App, Arg, ArgMatches, SubCommand};
use itertools::Itertools;
use simple_error::SimpleResult;
use std::collections::HashMap;

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

    async fn load(&self, ids: &[i64]) -> Result<HashMap<i64, Self::Value>, Self::Error> {
        let query = format!(
            "
        SELECT *
        FROM systems
        WHERE id in ({})
        ",
            ids.iter().join(",")
        );
        Ok(sqlx::query_as(&query)
            .fetch(POOL.get().unwrap())
            .map_ok(|system: System| (system.id, system))
            .try_collect()
            .await?)
    }
}

pub struct GameLoader;

#[async_trait]
impl Loader<i64> for GameLoader {
    type Value = Game;
    type Error = FieldError;

    async fn load(&self, ids: &[i64]) -> Result<HashMap<i64, Self::Value>, Self::Error> {
        let query = format!(
            "
        SELECT *
        FROM games
        WHERE id in ({})
        ",
            ids.iter().join(",")
        );
        Ok(sqlx::query_as(&query)
            .fetch(POOL.get().unwrap())
            .map_ok(|game: Game| (game.id, game))
            .try_collect()
            .await?)
    }
}

#[ComplexObject]
impl Game {
    async fn system(&self, ctx: &Context<'_>) -> Result<Option<System>> {
        Ok(ctx
            .data_unchecked::<DataLoader<SystemLoader>>()
            .load_one(self.system_id)
            .await?)
    }
}

pub struct RomLoader;

#[async_trait]
impl Loader<i64> for RomLoader {
    type Value = Rom;
    type Error = FieldError;

    async fn load(&self, ids: &[i64]) -> Result<HashMap<i64, Self::Value>, Self::Error> {
        let query = format!(
            "
        SELECT *
        FROM roms
        WHERE id in ({})
        ",
            ids.iter().join(",")
        );
        Ok(sqlx::query_as(&query)
            .fetch(POOL.get().unwrap())
            .map_ok(|rom: Rom| (rom.id, rom))
            .try_collect()
            .await?)
    }
}

#[ComplexObject]
impl Rom {
    async fn game(&self, ctx: &Context<'_>) -> Result<Option<Game>> {
        Ok(ctx
            .data_unchecked::<DataLoader<GameLoader>>()
            .load_one(self.game_id)
            .await?)
    }
}

pub struct RomfileLoader;

#[async_trait]
impl Loader<i64> for RomfileLoader {
    type Value = Romfile;
    type Error = FieldError;

    async fn load(&self, ids: &[i64]) -> Result<HashMap<i64, Self::Value>, Self::Error> {
        let query = format!(
            "
        SELECT *
        FROM romfiles
        WHERE id in ({})
        ",
            ids.iter().join(",")
        );
        Ok(sqlx::query_as(&query)
            .fetch(POOL.get().unwrap())
            .map_ok(|romfile: Romfile| (romfile.id, romfile))
            .try_collect()
            .await?)
    }
}

struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn systems(&self) -> Result<Vec<System>> {
        Ok(find_systems(POOL.get().unwrap()).await)
    }

    async fn system(&self, ctx: &Context<'_>, id: i64) -> Result<Option<System>> {
        Ok(ctx
            .data_unchecked::<DataLoader<SystemLoader>>()
            .load_one(id)
            .await?)
    }

    async fn games(&self, system_id: i64) -> Result<Vec<Game>> {
        Ok(find_games_by_system_id(POOL.get().unwrap(), system_id).await)
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

pub async fn main(matches: &ArgMatches<'_>) -> SimpleResult<()> {
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
    Ok(())
}
