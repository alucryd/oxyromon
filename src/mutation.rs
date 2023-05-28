use super::config::{add_to_list, remove_from_list, set_bool, set_string};
use super::server::POOL;
use super::validator::*;
use async_graphql::{Object, Result};

pub struct Mutation;

#[Object]
impl Mutation {
    async fn add_to_list(&self, key: String, value: String) -> Result<bool> {
        log::debug!("add to list");
        log::debug!("{}", key);
        log::debug!("{}", value);
        add_to_list(
            &mut POOL.get().unwrap().acquire().await.unwrap(),
            &key,
            &value,
        )
        .await;
        Ok(true)
    }

    async fn remove_from_list(&self, key: String, value: String) -> Result<bool> {
        log::debug!("remove from list");
        log::debug!("{}", key);
        log::debug!("{}", value);
        remove_from_list(
            &mut POOL.get().unwrap().acquire().await.unwrap(),
            &key,
            &value,
        )
        .await;
        Ok(true)
    }

    async fn set_bool(&self, key: String, value: bool) -> Result<bool> {
        log::debug!("set bool");
        log::debug!("{}", key);
        log::debug!("{}", value);
        set_bool(
            &mut POOL.get().unwrap().acquire().await.unwrap(),
            &key,
            value,
        )
        .await;
        Ok(true)
    }

    async fn set_prefer_regions(
        &self,
        #[graphql(validator(custom = "PreferRegionValidator::new()"))] value: String,
    ) -> Result<bool> {
        log::debug!("set prefer regions");
        log::debug!("{}", value);
        set_string(
            &mut POOL.get().unwrap().acquire().await.unwrap(),
            "PREFER_REGIONS",
            &value,
        )
        .await;
        Ok(true)
    }

    async fn set_prefer_versions(
        &self,
        #[graphql(validator(custom = "PreferVersionValidator::new()"))] value: String,
    ) -> Result<bool> {
        log::debug!("set prefer versions");
        log::debug!("{}", value);
        set_string(
            &mut POOL.get().unwrap().acquire().await.unwrap(),
            "PREFER_VERSIONS",
            &value,
        )
        .await;
        Ok(true)
    }
}
