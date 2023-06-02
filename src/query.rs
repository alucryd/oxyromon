use super::database::*;
use super::model::*;
use super::server::POOL;
use async_graphql::dataloader::{DataLoader, Loader};
use async_graphql::{ComplexObject, Context, Error, Object, Result};
use async_trait::async_trait;
use futures::stream::TryStreamExt;
use itertools::Itertools;
use num_traits::FromPrimitive;
use shiratsu_naming::naming::nointro::{NoIntroName, NoIntroToken};
use shiratsu_naming::naming::TokenizedName;
use shiratsu_naming::region::Region;
use std::collections::HashMap;

#[ComplexObject]
impl System {
    async fn header(&self) -> Result<Option<Header>> {
        Ok(
            find_header_by_system_id(&mut POOL.get().unwrap().acquire().await.unwrap(), self.id)
                .await,
        )
    }
}

#[ComplexObject]
impl Game {
    async fn system(&self, ctx: &Context<'_>) -> Result<Option<System>> {
        ctx.data_unchecked::<DataLoader<SystemLoader>>()
            .load_one(self.system_id)
            .await
    }
}

#[ComplexObject]
impl Rom {
    async fn game(&self, ctx: &Context<'_>) -> Result<Option<Game>> {
        ctx.data_unchecked::<DataLoader<GameLoader>>()
            .load_one(self.game_id)
            .await
    }

    async fn romfile(&self, ctx: &Context<'_>) -> Result<Option<Romfile>> {
        Ok(match self.romfile_id {
            Some(romfile_id) => {
                ctx.data_unchecked::<DataLoader<RomfileLoader>>()
                    .load_one(romfile_id)
                    .await?
            }
            None => None,
        })
    }

    async fn ignored(&self, ctx: &Context<'_>, system_id: i64) -> Result<bool> {
        let system_loader = ctx.data_unchecked::<DataLoader<SystemLoader>>();
        let system = system_loader.load_one(system_id).await?.unwrap();

        if !system.arcade || self.parent_id.is_none() {
            return Ok(false);
        }

        let merging = Merging::from_i64(system.merging).unwrap();
        let ignored = match merging {
            Merging::Split => true,
            Merging::NonMerged | Merging::Merged => {
                let sql = format!(
                    "
                        SELECT g.bios
                        FROM roms AS r
                        JOIN games AS g ON r.game_id = g.id
                        WHERE r.id = {};
                    ",
                    self.parent_id.unwrap()
                );
                let row: (bool,) = sqlx::query_as(&sql)
                    .fetch_one(&mut POOL.get().unwrap().acquire().await.unwrap().detach())
                    .await?;
                row.0
            }
            Merging::FullNonMerged | Merging::FullMerged => false,
        };
        Ok(ignored)
    }
}

pub struct SystemLoader;

#[async_trait]
impl Loader<i64> for SystemLoader {
    type Value = System;
    type Error = Error;

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
            .fetch(&mut POOL.get().unwrap().acquire().await.unwrap().detach())
            .map_ok(|system: System| (system.id, system))
            .try_collect()
            .await?)
    }
}

pub struct GameLoader;

#[async_trait]
impl Loader<i64> for GameLoader {
    type Value = Game;
    type Error = Error;

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
            .fetch(&mut POOL.get().unwrap().acquire().await.unwrap().detach())
            .map_ok(|game: Game| (game.id, game))
            .try_collect()
            .await?)
    }
}

pub struct RomfileLoader;

#[async_trait]
impl Loader<i64> for RomfileLoader {
    type Value = Romfile;
    type Error = Error;

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
            .fetch(&mut POOL.get().unwrap().acquire().await.unwrap().detach())
            .map_ok(|romfile: Romfile| (romfile.id, romfile))
            .try_collect()
            .await?)
    }
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn settings(&self) -> Result<Vec<Setting>> {
        log::debug!("query::get settings()");
        Ok(find_settings(&mut POOL.get().unwrap().acquire().await.unwrap()).await)
    }

    async fn systems(&self) -> Result<Vec<System>> {
        Ok(find_systems(&mut POOL.get().unwrap().acquire().await.unwrap()).await)
    }

    async fn games(&self, system_id: i64) -> Result<Vec<Game>> {
        Ok(
            find_games_by_system_id(&mut POOL.get().unwrap().acquire().await.unwrap(), system_id)
                .await,
        )
    }

    async fn game_information(&self, game_name: String) -> Result<GameInformation> {
        let mut title: String = String::new();
        let mut regions: Vec<String> = Vec::new();
        let mut languages: Vec<String> = Vec::new();
        let mut release: Option<String> = None;
        let mut flags: Vec<String> = Vec::new();

        if let Ok(name) = NoIntroName::try_parse(&game_name) {
            for token in name.iter() {
                if let NoIntroToken::Title(parsed_title) = token {
                    title = parsed_title.to_string();
                } else if let NoIntroToken::Region(_, parsed_regions) = token {
                    regions.append(
                        &mut Region::to_normalized_region_string(parsed_regions)
                            .split("-")
                            .map(|region| region.to_string())
                            .collect_vec(),
                    );
                } else if let NoIntroToken::Languages(parsed_languages) = token {
                    languages.append(
                        &mut parsed_languages
                            .iter()
                            .map(|(language, _)| language.to_string())
                            .collect_vec(),
                    );
                } else if let NoIntroToken::Release(parsed_release, _) = token {
                    release = Some(parsed_release.to_string());
                } else if let NoIntroToken::Flag(_, parsed_flags) = token {
                    flags.append(
                        &mut parsed_flags
                            .split(", ")
                            .map(|flag| flag.to_string())
                            .collect_vec(),
                    );
                }
            }
        }
        Ok(GameInformation {
            title,
            regions,
            languages,
            release,
            flags,
        })
    }

    async fn roms(&self, game_id: i64) -> Result<Vec<Rom>> {
        Ok(
            find_roms_by_game_id_parents(
                &mut POOL.get().unwrap().acquire().await.unwrap(),
                game_id,
            )
            .await,
        )
    }

    async fn total_original_size(&self, system_id: i64) -> Result<i64> {
        let sql = format!(
            "
                SELECT COALESCE(SUM(r.size), 0)
                FROM roms AS r
                JOIN games AS g ON r.game_id = g.id
                WHERE r.romfile_id IS NOT NULL
                AND g.system_id = {};
            ",
            system_id
        );
        let row: (i64,) = sqlx::query_as(&sql)
            .fetch_one(&mut POOL.get().unwrap().acquire().await.unwrap().detach())
            .await?;
        Ok(row.0)
    }

    async fn one_region_original_size(&self, system_id: i64) -> Result<i64> {
        let sql = format!(
            "
                SELECT COALESCE(SUM(r.size), 0)
                FROM roms AS r
                JOIN games AS g ON r.game_id = g.id
                WHERE r.romfile_id IS NOT NULL
                AND g.sorting = 1
                AND g.system_id = {};
            ",
            system_id
        );
        let row: (i64,) = sqlx::query_as(&sql)
            .fetch_one(&mut POOL.get().unwrap().acquire().await.unwrap().detach())
            .await?;
        Ok(row.0)
    }

    async fn total_actual_size(&self, system_id: i64) -> Result<i64> {
        let sql = format!(
            "
                SELECT COALESCE(SUM(rf.size), 0)
                FROM romfiles AS rf
                WHERE rf.id IN (
                    SELECT DISTINCT(r.romfile_id) FROM roms AS r
                    JOIN games AS g ON r.game_id = g.id
                    WHERE r.romfile_id IS NOT NULL
                    AND g.system_id = {}
                );
            ",
            system_id
        );
        let row: (i64,) = sqlx::query_as(&sql)
            .fetch_one(&mut POOL.get().unwrap().acquire().await.unwrap().detach())
            .await?;
        Ok(row.0)
    }

    async fn one_region_actual_size(&self, system_id: i64) -> Result<i64> {
        let sql = format!(
            "
                SELECT COALESCE(SUM(rf.size), 0)
                FROM romfiles AS rf
                WHERE rf.id IN (
                    SELECT DISTINCT(r.romfile_id) FROM roms AS r
                    JOIN games AS g ON r.game_id = g.id
                    WHERE r.romfile_id IS NOT NULL
                    AND g.sorting = 1
                    AND g.system_id = {}
                );
            ",
            system_id
        );
        let row: (i64,) = sqlx::query_as(&sql)
            .fetch_one(&mut POOL.get().unwrap().acquire().await.unwrap().detach())
            .await?;
        Ok(row.0)
    }
}
