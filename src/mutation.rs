use super::config::{add_to_list, remove_from_list, set_bool, set_directory, set_string};
use super::server::POOL;
use super::validator::*;
use async_graphql::{Object, Result};

pub struct Mutation;

#[Object]
impl Mutation {
    async fn add_to_list(&self, key: String, value: String) -> Result<bool> {
        log::debug!("mutation::add_to_list({}, {})", &key, &value);
        add_to_list(
            &mut POOL.get().unwrap().acquire().await.unwrap(),
            &key,
            &value,
        )
        .await;
        Ok(true)
    }

    async fn remove_from_list(&self, key: String, value: String) -> Result<bool> {
        log::debug!("mutation::remove_to_list({}, {})", &key, &value);
        remove_from_list(
            &mut POOL.get().unwrap().acquire().await.unwrap(),
            &key,
            &value,
        )
        .await;
        Ok(true)
    }

    async fn set_bool(&self, key: String, value: bool) -> Result<bool> {
        log::debug!("mutation::set_bool({}, {})", &key, &value);
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
        log::debug!("mutation::set_prefer_regions({})", &value);
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
        log::debug!("mutation::set_prefer_versions({})", &value);
        set_string(
            &mut POOL.get().unwrap().acquire().await.unwrap(),
            "PREFER_VERSIONS",
            &value,
        )
        .await;
        Ok(true)
    }

    async fn set_subfolder_scheme(
        &self,
        key: String,
        #[graphql(validator(custom = "SubfolderSchemeValidator::new()"))] value: String,
    ) -> Result<bool> {
        log::debug!("mutation::set_subfolder_scheme({}, {})", &key, &value);
        set_string(
            &mut POOL.get().unwrap().acquire().await.unwrap(),
            &key,
            &value,
        )
        .await;
        Ok(true)
    }

    async fn set_directory(
        &self,
        key: String,
        #[graphql(validator(custom = "DirectoryValidator::new()"))] value: String,
    ) -> Result<bool> {
        log::debug!("mutation::set_directory({}, {})", &key, &value);
        set_directory(
            &mut POOL.get().unwrap().acquire().await.unwrap(),
            &key,
            &value,
        )
        .await;
        Ok(true)
    }
}
