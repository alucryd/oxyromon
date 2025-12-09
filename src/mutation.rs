use super::config::{add_to_list, remove_from_list, set_bool, set_directory, set_string};
use super::database::*;
use super::purge_systems::purge_system;
use super::validator::*;
use async_graphql::{Context, Object, Result};
use indicatif::ProgressBar;
use sqlx::SqlitePool;

pub struct Mutation;

#[Object]
impl Mutation {
    async fn add_to_list(&self, ctx: &Context<'_>, key: String, value: String) -> Result<bool> {
        log::debug!("mutation::add_to_list({}, {})", &key, &value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        add_to_list(&mut pool.acquire().await.unwrap(), &key, &value).await;
        Ok(true)
    }

    async fn remove_from_list(
        &self,
        ctx: &Context<'_>,
        key: String,
        value: String,
    ) -> Result<bool> {
        log::debug!("mutation::remove_to_list({}, {})", &key, &value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        remove_from_list(&mut pool.acquire().await.unwrap(), &key, &value).await;
        Ok(true)
    }

    async fn set_bool(&self, ctx: &Context<'_>, key: String, value: bool) -> Result<bool> {
        log::debug!("mutation::set_bool({}, {})", &key, &value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        set_bool(&mut pool.acquire().await.unwrap(), &key, value).await;
        Ok(true)
    }

    async fn set_prefer_regions(
        &self,
        ctx: &Context<'_>,
        #[graphql(validator(custom = "PreferRegionValidator::new()"))] value: String,
    ) -> Result<bool> {
        log::debug!("mutation::set_prefer_regions({})", &value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        set_string(&mut pool.acquire().await.unwrap(), "PREFER_REGIONS", &value).await;
        Ok(true)
    }

    async fn set_prefer_versions(
        &self,
        ctx: &Context<'_>,
        #[graphql(validator(custom = "PreferVersionValidator::new()"))] value: String,
    ) -> Result<bool> {
        log::debug!("mutation::set_prefer_versions({})", &value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        set_string(
            &mut pool.acquire().await.unwrap(),
            "PREFER_VERSIONS",
            &value,
        )
        .await;
        Ok(true)
    }

    async fn set_subfolder_scheme(
        &self,
        ctx: &Context<'_>,
        key: String,
        #[graphql(validator(custom = "SubfolderSchemeValidator::new()"))] value: String,
    ) -> Result<bool> {
        log::debug!("mutation::set_subfolder_scheme({}, {})", &key, &value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        set_string(&mut pool.acquire().await.unwrap(), &key, &value).await;
        Ok(true)
    }

    async fn set_directory(
        &self,
        ctx: &Context<'_>,
        key: String,
        #[graphql(validator(custom = "DirectoryValidator::new()"))] value: String,
    ) -> Result<bool> {
        log::debug!("mutation::set_directory({}, {})", &key, &value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        set_directory(&mut pool.acquire().await.unwrap(), &key, &value).await;
        Ok(true)
    }

    async fn purge_system(&self, ctx: &Context<'_>, system_id: i64) -> Result<bool> {
        log::debug!("mutation::purge_system({})", system_id);
        let pool = ctx.data_unchecked::<SqlitePool>();
        let mut connection = pool.acquire().await.unwrap();

        let system = find_system_by_id(&mut connection, system_id).await;
        let progress_bar = ProgressBar::hidden();

        purge_system(&mut connection, &progress_bar, &system)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        Ok(true)
    }
}
