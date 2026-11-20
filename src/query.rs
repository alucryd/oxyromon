use super::database::*;
use super::download_dats::find_redump_systems;
use super::model::*;
use async_graphql::dataloader::{DataLoader, Loader};
use async_graphql::{ComplexObject, Context, Error, Object, Result};
use futures::stream::TryStreamExt;
use itertools::Itertools;
use num_traits::FromPrimitive;
use shiratsu_naming::naming::TokenizedName;
use shiratsu_naming::naming::nointro::{NoIntroName, NoIntroToken};
use shiratsu_naming::region::Region;
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, FromRow, SqlitePool};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::task::spawn_blocking;

async fn system_size(
    ctx: &Context<'_>,
    system_id: i64,
    actual: bool,
    one_region: bool,
) -> Result<i64> {
    let pool = ctx.data_unchecked::<SqlitePool>();
    let sorting = if one_region {
        "AND g.sorting = 1\n                    "
    } else {
        ""
    };
    if actual {
        // The store's index is asked for before anything else — before a
        // connection, above all: the batch that serves the key needs one,
        // and a field holding the pool's five while it waits would starve
        // the batch into the acquire timeout. The key still goes in on the
        // field's first poll, so every size field of a request reaches the
        // loader in the same breath and one read serves them all.
        let store = ctx
            .data_unchecked::<DataLoader<StoreIndexLoader>>()
            .load_one(system_id)
            .await
            .ok()
            .flatten();
        let mut connection = pool.acquire().await.unwrap();
        let sql = format!(
            "
                SELECT *
                FROM romfiles AS rf
                WHERE rf.id IN (
                    SELECT DISTINCT(r.romfile_id) FROM roms AS r
                    JOIN games AS g ON r.game_id = g.id
                    WHERE r.romfile_id IS NOT NULL
                    {}AND g.system_id = {}
                );
            ",
            sorting, system_id
        );
        let romfiles: Vec<Romfile> = sqlx::query_as(AssertSqlSafe(sql.as_str()))
            .fetch_all(&mut *connection)
            .await?;
        return Ok(crate::ore::actual_size_loaded(&mut connection, &romfiles, store).await? as i64);
    }
    let mut connection = pool.acquire().await.unwrap();
    let sql = format!(
        "
            SELECT COALESCE(SUM(r.size), 0)
            FROM roms AS r
            JOIN games AS g ON r.game_id = g.id
            WHERE r.romfile_id IS NOT NULL
            {}AND g.system_id = {};
        ",
        sorting, system_id
    );
    let row: (i64,) = sqlx::query_as(AssertSqlSafe(sql.as_str()))
        .fetch_one(&mut *connection)
        .await?;
    Ok(row.0)
}

/// The advanced stats of the system's ORE store, `None` when it uses none:
/// what its dedup saved and what its parity costs, read off its index.
/// Like the sizes, the field asks for the store by system id and asks
/// first: one request's stats fields share the one index read.
async fn store_stats_of(ctx: &Context<'_>, system_id: i64) -> Option<ore_rs::StoreStats> {
    let store = ctx
        .data_unchecked::<DataLoader<StoreIndexLoader>>()
        .load_one(system_id)
        .await
        .ok()?;
    Some(store?.index.stats())
}

#[ComplexObject]
impl System {
    async fn header(&self, ctx: &Context<'_>) -> Result<Option<Header>> {
        let pool = ctx.data_unchecked::<SqlitePool>();
        Ok(find_header_by_system_id(&mut pool.acquire().await.unwrap(), self.id).await)
    }
}

#[ComplexObject]
impl Game {
    /// Human readable title, or null when it is identical to `name` and the
    /// client can simply fall back to it.
    async fn description(&self) -> Option<&str> {
        if self.description == self.name {
            None
        } else {
            Some(&self.description)
        }
    }

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
        let pool = ctx.data_unchecked::<SqlitePool>();
        let system_loader = ctx.data_unchecked::<DataLoader<SystemLoader>>();
        let system = system_loader.load_one(system_id).await?.unwrap();

        let game_loader = ctx.data_unchecked::<DataLoader<GameLoader>>();
        let game = game_loader.load_one(self.game_id).await?.unwrap();

        if game.sorting == Sorting::Ignored as i64 {
            return Ok(true);
        }

        if !system.arcade || self.parent_id.is_none() {
            return Ok(false);
        }

        let merging = Merging::from_i64(system.merging).unwrap();
        let ignored = match merging {
            Merging::Split | Merging::Merged => true,
            Merging::NonMerged => {
                let sql = format!(
                    "
                        SELECT g.bios
                        FROM roms AS r
                        JOIN games AS g ON r.game_id = g.id
                        WHERE r.id = {};
                    ",
                    self.parent_id.unwrap()
                );
                let row: (bool,) = sqlx::query_as(AssertSqlSafe(sql.as_str()))
                    .fetch_one(&mut *pool.acquire().await.unwrap())
                    .await?;
                row.0
            }
            Merging::FullNonMerged | Merging::FullMerged => false,
        };
        Ok(ignored)
    }
}

pub struct IdLoader<T: TableRow> {
    pub pool: SqlitePool,
    _marker: PhantomData<T>,
}

impl<T: TableRow> IdLoader<T> {
    pub fn new(pool: SqlitePool) -> Self {
        IdLoader {
            pool,
            _marker: PhantomData,
        }
    }
}

pub type SystemLoader = IdLoader<System>;
pub type GameLoader = IdLoader<Game>;
pub type RomfileLoader = IdLoader<Romfile>;

pub(crate) trait TableRow:
    for<'r> FromRow<'r, SqliteRow> + Clone + Send + Sync + Unpin + 'static
{
    const TABLE: &'static str;
    // The `SELECT` list. Plain tables use `*`, but a table may carry computed
    // columns (e.g. `System`'s completion counts) that are not real columns and
    // so are absent from `*` — the loader would then fail to map the row.
    const SELECT: &'static str = "*";
    fn row_id(&self) -> i64;
}

impl TableRow for System {
    const TABLE: &'static str = "systems";
    // Must match the count subqueries the System-mapping queries in database.rs
    // select, so a row loaded this way maps to the full `System`.
    const SELECT: &'static str = "systems.*, \
        (SELECT COUNT(*) FROM games WHERE games.system_id = systems.id AND games.completion = 2 AND games.sorting != 2) AS games_complete, \
        (SELECT COUNT(*) FROM games WHERE games.system_id = systems.id AND games.sorting != 2) AS games_total";
    fn row_id(&self) -> i64 {
        self.id
    }
}

impl TableRow for Game {
    const TABLE: &'static str = "games";
    fn row_id(&self) -> i64 {
        self.id
    }
}

impl TableRow for Romfile {
    const TABLE: &'static str = "romfiles";
    fn row_id(&self) -> i64 {
        self.id
    }
}

impl<T: TableRow + 'static> Loader<i64> for IdLoader<T> {
    type Value = T;
    type Error = Error;

    async fn load(&self, ids: &[i64]) -> Result<HashMap<i64, Self::Value>, Self::Error> {
        let sql = format!(
            "
        SELECT {}
        FROM {}
        WHERE id in ({})
        ",
            T::SELECT,
            T::TABLE,
            ids.iter().join(",")
        );
        Ok(sqlx::query_as(AssertSqlSafe(sql.as_str()))
            .fetch(&mut *self.pool.acquire().await.unwrap())
            .map_ok(|row: T| (row.row_id(), row))
            .try_collect()
            .await?)
    }
}

/// A system's store index, loaded once per GraphQL request: a sizes query
/// reads the same store through every one of its fields, and a big store's
/// index is no small read. Keyed by system, with the store's location
/// resolved here: a field holding its id needs no round trip before it can
/// submit its key, and submitting before any other await is what lets a
/// request batch the reads of all its fields into one. A key that fails to
/// load — a system with no store beneath it above all — simply has no
/// value, as the sizes and the stats treat an unreadable store as an absent one.
#[derive(Clone)]
pub struct StoreIndexLoader {
    pool: SqlitePool,
    /// Indexes read from disk: requests only add to it, the tests below
    /// read it to prove they asked for the same store only once.
    #[cfg_attr(not(test), allow(dead_code))]
    loads: Arc<AtomicUsize>,
    /// Reads the preload served: proves the sizes found the index they
    /// asked for early, not a store whose path disagreed with the system's.
    #[cfg_attr(not(test), allow(dead_code))]
    hits: Arc<AtomicUsize>,
}

impl StoreIndexLoader {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            loads: Arc::new(AtomicUsize::new(0)),
            hits: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// How many indexes this loader read from disk: one per store, however
    /// many resolvers of the request asked for it.
    #[cfg(test)]
    pub fn loads(&self) -> usize {
        self.loads.load(Ordering::Relaxed)
    }

    /// How many times the sizes took the preloaded index: once is enough
    /// to prove a store's read served more than the field that asked for it.
    #[cfg(test)]
    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }
}

/// A store's index, with the store it was read from: sizes that find a
/// different store than the system names read that one themselves.
pub struct StoreIndex {
    pub store: PathBuf,
    pub index: Arc<ore_rs::Index>,
    hits: Arc<AtomicUsize>,
}

impl StoreIndex {
    /// The index of `store` when it is the preloaded one, the disk's
    /// otherwise: a thin ORE beneath another system's store — a grouped
    /// subsystem, or a misplaced file — weighs what lies before it.
    pub async fn index_for(&self, store: &Path) -> Option<Arc<ore_rs::Index>> {
        if self.store == store {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Some(self.index.clone());
        }
        let store = store.to_path_buf();
        spawn_blocking(move || ore_rs::Index::load(&store).ok().map(Arc::new))
            .await
            .ok()
            .flatten()
    }
}

impl Loader<i64> for StoreIndexLoader {
    type Value = Arc<StoreIndex>;
    type Error = Error;

    async fn load(&self, systems: &[i64]) -> Result<HashMap<i64, Self::Value>, Self::Error> {
        let mut connection = self.pool.acquire().await?;
        let mut stores = Vec::new();
        for system_id in systems.iter().unique() {
            if let Some(system) = find_system_by_id_opt(&mut connection, *system_id).await
                && let Ok(store) = crate::ore::system_store(&mut connection, &system).await
            {
                stores.push((*system_id, store));
            }
        }
        // The parse reads no database: the connection goes back to the pool
        // before the multi-second read, not after it. Failing the batch is
        // graceful too — the fields treat a refused index as an absent one
        // and read their stores off disk.
        drop(connection);
        let loads = self.loads.clone();
        let hits = self.hits.clone();
        Ok(spawn_blocking(move || {
            let mut indexes = HashMap::new();
            for (system_id, store) in stores {
                if let Ok(index) = ore_rs::Index::load(&store) {
                    loads.fetch_add(1, Ordering::Relaxed);
                    indexes.insert(
                        system_id,
                        Arc::new(StoreIndex {
                            store,
                            index: Arc::new(index),
                            hits: hits.clone(),
                        }),
                    );
                }
            }
            indexes
        })
        .await?)
    }
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn version(&self) -> Result<String> {
        Ok(env!("CARGO_PKG_VERSION").to_string())
    }

    async fn dependencies(&self) -> Result<Vec<Dependency>> {
        Ok(super::info::dependencies()
            .await
            .into_iter()
            .map(|(name, result)| Dependency {
                name: name.to_string(),
                version: result.ok(),
            })
            .collect())
    }

    /// Redump systems the server can fetch: those not yet in the database, or
    /// with `update`, those already in it whose DATs can be refreshed.
    async fn downloadable_systems(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = false)] update: bool,
    ) -> Result<Vec<String>> {
        let pool = ctx.data_unchecked::<SqlitePool>();
        Ok(find_redump_systems(&mut pool.acquire().await.unwrap(), update).await)
    }

    async fn system_count(&self, ctx: &Context<'_>) -> Result<i64> {
        let pool = ctx.data_unchecked::<SqlitePool>();
        Ok(count_systems(&mut pool.acquire().await.unwrap()).await)
    }

    async fn game_count(&self, ctx: &Context<'_>) -> Result<i64> {
        let pool = ctx.data_unchecked::<SqlitePool>();
        Ok(count_games(&mut pool.acquire().await.unwrap()).await)
    }

    async fn rom_count(&self, ctx: &Context<'_>) -> Result<i64> {
        let pool = ctx.data_unchecked::<SqlitePool>();
        Ok(count_roms(&mut pool.acquire().await.unwrap()).await)
    }

    async fn settings(&self, ctx: &Context<'_>) -> Result<Vec<Setting>> {
        log::debug!("query::get settings()");
        let pool = ctx.data_unchecked::<SqlitePool>();
        Ok(find_settings(&mut pool.acquire().await.unwrap(), None).await)
    }

    async fn system_settings(&self, ctx: &Context<'_>, system_id: i64) -> Result<Vec<Setting>> {
        log::debug!("query::system_settings({})", system_id);
        let pool = ctx.data_unchecked::<SqlitePool>();
        let mut conn = pool.acquire().await.unwrap();
        let global = find_settings(&mut conn, None).await;
        let overrides = find_settings(&mut conn, Some(system_id)).await;
        let mut merged: HashMap<String, Setting> =
            global.into_iter().map(|s| (s.key.clone(), s)).collect();
        for setting in overrides {
            merged.insert(setting.key.clone(), setting);
        }
        let mut result: Vec<Setting> = merged.into_values().collect();
        result.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(result)
    }

    async fn systems(&self, ctx: &Context<'_>) -> Result<Vec<System>> {
        let pool = ctx.data_unchecked::<SqlitePool>();
        Ok(find_systems(&mut pool.acquire().await.unwrap()).await)
    }

    /// Games of a system, ordered by name.
    ///
    /// `offset` and `limit` let a client pull a large system in pieces rather
    /// than waiting on one multi megabyte response; omitting `limit` returns
    /// everything.
    async fn games(
        &self,
        ctx: &Context<'_>,
        system_id: i64,
        offset: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<Game>> {
        let pool = ctx.data_unchecked::<SqlitePool>();
        let connection = &mut pool.acquire().await.unwrap();
        Ok(match limit {
            Some(limit) => {
                find_games_by_system_id_paged(connection, system_id, offset.unwrap_or(0), limit)
                    .await
            }
            None => find_games_by_system_id(connection, system_id).await,
        })
    }

    async fn game_information(&self, game_name: String) -> Result<GameInformation> {
        let mut title: String = String::new();
        let mut regions: Vec<String> = vec![];
        let mut languages: Vec<String> = vec![];
        let mut release: Option<String> = None;
        let mut flags: Vec<String> = vec![];

        if let Ok(name) = NoIntroName::try_parse(&game_name) {
            for token in name.iter() {
                if let NoIntroToken::Title(parsed_title) = token {
                    title = parsed_title.to_string();
                } else if let NoIntroToken::Region {
                    region_strings: _,
                    regions: parsed_regions,
                } = token
                {
                    regions.append(
                        &mut Region::to_normalized_region_string(parsed_regions)
                            .split('-')
                            .map(|region| region.to_string())
                            .collect_vec(),
                    );
                } else if let NoIntroToken::Languages(parsed_languages) = token {
                    languages.append(
                        &mut parsed_languages
                            .iter()
                            .map(|l| l.code.to_string())
                            .collect_vec(),
                    );
                } else if let NoIntroToken::Release { status, number: _ } = token {
                    release = Some(status.to_string());
                } else if let NoIntroToken::Flag { flag_type: _, flag } = token {
                    flags.append(&mut flag.split(", ").map(|flag| flag.to_string()).collect_vec());
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

    async fn roms(&self, ctx: &Context<'_>, game_id: i64) -> Result<Vec<Rom>> {
        let pool = ctx.data_unchecked::<SqlitePool>();
        Ok(find_roms_by_game_id_parents(&mut pool.acquire().await.unwrap(), game_id).await)
    }

    async fn total_original_size(&self, ctx: &Context<'_>, system_id: i64) -> Result<i64> {
        system_size(ctx, system_id, false, false).await
    }

    async fn one_region_original_size(&self, ctx: &Context<'_>, system_id: i64) -> Result<i64> {
        system_size(ctx, system_id, false, true).await
    }

    async fn total_actual_size(&self, ctx: &Context<'_>, system_id: i64) -> Result<i64> {
        system_size(ctx, system_id, true, false).await
    }

    async fn one_region_actual_size(&self, ctx: &Context<'_>, system_id: i64) -> Result<i64> {
        system_size(ctx, system_id, true, true).await
    }

    async fn store_dedup_saved(&self, ctx: &Context<'_>, system_id: i64) -> Result<Option<i64>> {
        Ok(store_stats_of(ctx, system_id)
            .await
            .map(|stats| stats.dedup_saved as i64))
    }

    async fn store_parity_cost(&self, ctx: &Context<'_>, system_id: i64) -> Result<Option<i64>> {
        Ok(store_stats_of(ctx, system_id)
            .await
            .map(|stats| stats.parity as i64))
    }
}
