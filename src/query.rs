use super::database::*;
use super::download_dats::find_redump_systems;
use super::mimetype::ORE_EXTENSION;
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
use tokio::sync::OnceCell;
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
        let romfiles = find_system_romfiles(&mut connection, system_id, one_region).await?;
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

/// The advanced stats of the system's OREs in their store: what they save
/// by dedup and what their parity costs, each game paying its own part.
/// A store shared by grouped systems gives every system its own numbers,
/// which add up across the group to the store's whole. Like the sizes,
/// the field asks for the store by system id and asks first: one
/// request's stats fields share the one index read — and, through the
/// index, the one walk of the ORE headers that computes the savings.
/// A system the preload cannot answer for — a store that would not open,
/// or no store where the settings name one — is asked of the disk, the
/// way the sizes weigh a misplaced file: every ORE of every store it
/// actually lies above is counted, as the MCP does. The walk is not
/// shared there; the edge is rare and the answer is the same number.
async fn store_savings_of(ctx: &Context<'_>, system_id: i64) -> Option<ore_rs::Savings> {
    let pool = ctx.data_unchecked::<SqlitePool>();
    let store = ctx
        .data_unchecked::<DataLoader<StoreIndexLoader>>()
        .load_one(system_id)
        .await
        .ok()
        .flatten();
    if let Some(store) = store {
        return store.savings(system_id).await;
    }
    let mut connection = pool.acquire().await.ok()?;
    let romfiles = find_system_romfiles(&mut connection, system_id, false)
        .await
        .ok()?;
    crate::ore::store_savings_loaded(&mut connection, &romfiles, None).await
}

/// What the files of `romfiles` weigh, a thin ORE's its share of its store,
/// the system's store index asked of the request's loader first so one read
/// serves every share: [`actual_size_loaded`] for a game instead of a system.
async fn romfiles_size_loaded(
    ctx: &Context<'_>,
    system_id: i64,
    romfiles: Vec<Romfile>,
) -> Result<i64> {
    let pool = ctx.data_unchecked::<SqlitePool>();
    let store = ctx
        .data_unchecked::<DataLoader<StoreIndexLoader>>()
        .load_one(system_id)
        .await
        .ok()
        .flatten();
    let mut connection = pool.acquire().await.unwrap();
    Ok(crate::ore::actual_size_loaded(&mut connection, &romfiles, store).await? as i64)
}

/// What the OREs of `romfiles` weigh in full: each one's own bytes, its
/// share of its store, and what it would weigh detached, summed. The
/// request's loader answers for the system's own store; one beneath a
/// directory the preload never heard of — a grouped subsystem moved, a
/// misplaced file — reads itself, as the sizes do. A standalone ORE
/// counts its file: an own size with nothing shared and nothing saved.
/// `None` when no file of the game is an ORE at all, or none of them can
/// be weighed.
async fn game_ore_sizes(
    ctx: &Context<'_>,
    system_id: i64,
    romfiles: &[Romfile],
) -> Result<Option<(i64, i64, i64)>> {
    let pool = ctx.data_unchecked::<SqlitePool>();
    let rom_directory = crate::config::get_rom_directory(&mut pool.acquire().await.unwrap()).await;
    let paths: Vec<PathBuf> = romfiles
        .iter()
        .filter(|romfile| {
            Path::new(&romfile.path)
                .extension()
                .is_some_and(|extension| extension == ORE_EXTENSION)
        })
        .map(|romfile| rom_directory.join(&romfile.path))
        .collect();
    if paths.is_empty() {
        return Ok(None);
    }
    let store = ctx
        .data_unchecked::<DataLoader<StoreIndexLoader>>()
        .load_one(system_id)
        .await
        .ok()
        .flatten();
    // The headers read off disk, in one pass: what each ORE is, and the
    // distinct stores a thin one names.
    let (weights, stores) = {
        let paths = paths.clone();
        spawn_blocking(move || {
            let weights = ore_rs::ore_weights(&paths.iter().map(AsRef::as_ref).collect::<Vec<_>>());
            let stores: Vec<PathBuf> = weights
                .iter()
                .filter_map(|weight| match weight {
                    ore_rs::OreWeight::Thin { store, .. } => Some(store.clone()),
                    _ => None,
                })
                .unique()
                .collect();
            (weights, stores)
        })
        .await?
    };
    // One index per store: the preload answers its own, whatever the
    // rest are; the others read themselves.
    let mut indexes: Vec<Arc<ore_rs::Index>> = vec![];
    for store_path in &stores {
        // the preload answers its own store; the rest read themselves
        if let Some(index) = &store
            && index.store == *store_path
        {
            indexes.push(index.index.clone());
            continue;
        }
        let store_path = store_path.clone();
        if let Some(index) =
            spawn_blocking(move || ore_rs::Index::load(&store_path).ok().map(Arc::new)).await?
        {
            indexes.push(index);
        }
    }
    let totals = spawn_blocking(move || {
        let refs: Vec<&ore_rs::Index> = indexes.iter().map(AsRef::as_ref).collect();
        let paths: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        let sizes = ore_rs::Index::weighed_sizes(&paths, &weights, &refs);
        let weighed = sizes.iter().filter_map(|size| size.as_ref()).count();
        // A game only shows its three figures when every file answered:
        // half a total misleads where no total simply does not.
        (weighed == sizes.len()).then(|| {
            sizes
                .iter()
                .map(|size| size.unwrap())
                .fold((0i64, 0i64, 0i64), |a, size| {
                    (
                        a.0 + i64::try_from(size.own).unwrap_or(i64::MAX),
                        a.1 + i64::try_from(size.share).unwrap_or(i64::MAX),
                        a.2 + i64::try_from(size.standalone).unwrap_or(i64::MAX),
                    )
                })
        })
    })
    .await?;
    Ok(totals)
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
    /// Savings walks run: proves the stats fields of one request shared
    /// the one walk of the system's ORE headers, rather than each paying.
    #[cfg_attr(not(test), allow(dead_code))]
    walks: Arc<AtomicUsize>,
}

impl StoreIndexLoader {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            loads: Arc::new(AtomicUsize::new(0)),
            hits: Arc::new(AtomicUsize::new(0)),
            walks: Arc::new(AtomicUsize::new(0)),
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

    /// How many savings walks ran: one per system whose stats were asked,
    /// however many of its fields asked.
    #[cfg(test)]
    pub fn walks(&self) -> usize {
        self.walks.load(Ordering::Relaxed)
    }
}

/// A store's index, with the store it was read from: sizes that find a
/// different store than the system names read that one themselves. It
/// also remembers the system's savings once they have been asked for:
/// the two stats fields of one request reach this value through the same
/// key, and the walk that answers the first serves the second too.
pub struct StoreIndex {
    pub store: PathBuf,
    pub index: Arc<ore_rs::Index>,
    hits: Arc<AtomicUsize>,
    pool: SqlitePool,
    walks: Arc<AtomicUsize>,
    savings: OnceCell<Option<ore_rs::Savings>>,
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

    /// The savings of the system named by `system_id`, computed once and
    /// kept: the romfiles, the walk of the ORE headers, and the sum run
    /// once however many stats fields ask. A `None` is kept as readily as
    /// a number — files that live in no readable store save and pay
    /// nothing, on every asking.
    async fn savings(self: &Arc<Self>, system_id: i64) -> Option<ore_rs::Savings> {
        *self
            .savings
            .get_or_init(|| async {
                let mut connection = self.pool.acquire().await.ok()?;
                let romfiles = find_system_romfiles(&mut connection, system_id, false)
                    .await
                    .ok()?;
                self.walks.fetch_add(1, Ordering::Relaxed);
                crate::ore::store_savings_loaded(&mut connection, &romfiles, Some(self.clone()))
                    .await
            })
            .await
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
        // graceful too — a field that reaches no store here weighs or walks
        // what lies under the system itself, off disk.
        drop(connection);
        let loads = self.loads.clone();
        let hits = self.hits.clone();
        let pool = self.pool.clone();
        let walks = self.walks.clone();
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
                            pool: pool.clone(),
                            walks: walks.clone(),
                            savings: OnceCell::new(),
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

    /// One system by id, through the loader the games' `system` field
    /// already uses: the info modal asks a single system its DAT and its
    /// counts, and fetching every system to pick a row out of the answer
    /// is not an answer.
    async fn system(&self, ctx: &Context<'_>, system_id: i64) -> Result<Option<System>> {
        ctx.data_unchecked::<DataLoader<SystemLoader>>()
            .load_one(system_id)
            .await
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
        Ok(store_savings_of(ctx, system_id)
            .await
            .map(|savings| savings.dedup_saved as i64))
    }

    async fn store_parity_cost(&self, ctx: &Context<'_>, system_id: i64) -> Result<Option<i64>> {
        Ok(store_savings_of(ctx, system_id)
            .await
            .map(|savings| savings.parity as i64))
    }

    /// The ORE store a system's thin OREs live in: its path and weight,
    /// what it saves and costs as a whole, this system's part of that,
    /// and every system sharing it. `None` when none exists above the
    /// system's directory: the info modal's store section, like the
    /// stats, is an extra an ordinary folder never shows.
    async fn system_store(&self, ctx: &Context<'_>, system_id: i64) -> Result<Option<StoreInfo>> {
        let pool = ctx.data_unchecked::<SqlitePool>();
        let connection = &mut *pool.acquire().await.unwrap();
        let Some(system) = find_system_by_id_opt(connection, system_id).await else {
            return Ok(None);
        };
        let Ok(store) = crate::ore::system_store(connection, &system).await else {
            return Ok(None);
        };
        if !store.is_dir() {
            return Ok(None);
        }
        // Every system whose directory holds this store: the group
        // `GROUP_SUBSYSTEMS` builds, this system named too. The directory
        // walk is disk work, but a system's directory is one stat per
        // system, and only the info modal asks.
        let mut systems = Vec::new();
        let mut shared = false;
        for other in find_systems(connection).await {
            if let Ok(other_store) = crate::ore::system_store(connection, &other).await
                && other_store == store
            {
                shared |= other.id == system_id;
                systems.push(other.name);
            }
        }
        let savings = if shared {
            let romfiles = find_system_romfiles(connection, system_id, false).await?;
            crate::ore::store_savings_loaded(connection, &romfiles, None).await
        } else {
            None
        };
        let stats = spawn_blocking({
            let store = store.clone();
            move || ore_rs::store_stats(&store)
        })
        .await
        .ok()
        .and_then(|stats| stats.ok());
        let size = spawn_blocking({
            let store = store.clone();
            move || ore_rs::store_size(&store)
        })
        .await
        .ok()
        .and_then(|size| size.ok())
        .unwrap_or(0);
        Ok(stats.map(|stats| StoreInfo {
            path: store.display().to_string(),
            size: size as i64,
            dedup_saved: stats.dedup_saved as i64,
            parity_cost: stats.parity as i64,
            system_dedup_saved: savings.map_or(0, |savings| savings.dedup_saved as i64),
            system_parity_cost: savings.map_or(0, |savings| savings.parity as i64),
            systems,
        }))
    }

    /// What one game weighs: what its DAT declares, what it weighs on
    /// disk, and — its files being thin OREs — what it owns, shares, and
    /// would weigh detached. Asked lazily by the info modal, the store
    /// index served by the same request loader the sizes share.
    async fn game_sizes(&self, ctx: &Context<'_>, game_id: i64) -> Result<GameSizes> {
        let pool = ctx.data_unchecked::<SqlitePool>();
        let connection = &mut *pool.acquire().await.unwrap();
        let romfiles = find_romfiles_by_game_id(connection, game_id).await;
        let original_size: i64 = sqlx::query_as::<_, (i64,)>(AssertSqlSafe(
            "SELECT COALESCE(SUM(size), 0) FROM roms WHERE game_id = ?",
        ))
        .bind(game_id)
        .fetch_one(&mut *connection)
        .await
        .map(|row| row.0)?;
        let game = find_game_by_id(connection, game_id).await;
        let actual_size = romfiles_size_loaded(ctx, game.system_id, romfiles.clone()).await?;
        let ore = game_ore_sizes(ctx, game.system_id, &romfiles).await?;
        Ok(GameSizes {
            original_size,
            actual_size,
            ore_own: ore.map(|ore| ore.0),
            ore_share: ore.map(|ore| ore.1),
            ore_standalone: ore.map(|ore| ore.2),
        })
    }
}
