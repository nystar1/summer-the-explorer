use std::sync::OnceLock;
use tokio::sync::Mutex;

use super::connection::{DbPool, create_pool};
use crate::utils::{config::Config, error::Result};

static SHARED_POOL: OnceLock<Mutex<Option<DbPool>>> = OnceLock::new();

pub struct ConnectionManager;

impl ConnectionManager {
    pub async fn get_shared_pool(config: &Config) -> Result<DbPool> {
        let pool_mutex = SHARED_POOL.get_or_init(|| Mutex::new(None));
        let mut pool_guard = pool_mutex.lock().await;
        
        if let Some(ref pool) = *pool_guard {
            return Ok(pool.clone());
        }

        let new_pool = create_pool(config).await?;
        *pool_guard = Some(new_pool.clone());
        Ok(new_pool)
    }

    pub async fn get_dedicated_pool(config: &Config) -> Result<DbPool> {
        create_pool(config).await
    }

    pub async fn clear_shared_pool() {
        if let Some(pool_mutex) = SHARED_POOL.get() {
            *pool_mutex.lock().await = None;
        }
    }
}
