use super::error::{ApiError, Result};
use serde::Deserialize;
use std::env;

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(clippy::struct_excessive_bools)] // :skull:
pub struct Config {
    pub database_url: String,
    pub journey_session_cookie: String,
    pub max_db_connections: u32,
    pub api_port: u16,
    pub first_sync_mode: bool,
    pub auto_sync_on_startup: bool,
    pub force_embedding_regen: Option<String>,
    pub skip_projects_sync: bool,
    pub skip_devlogs_sync: bool,
    pub skip_comments_sync: bool,
    pub skip_leaderboard_sync: bool,
    pub slack_token: String,
    pub embedding_cache_size: usize,
    pub embedding_cache_ttl_seconds: u64,
    pub embedding_max_concurrent_requests: usize,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Self {
            database_url: Self::get_required_env("DATABASE_URL")?,
            journey_session_cookie: Self::get_required_env("JOURNEY_SESSION_COOKIE")?,
            max_db_connections: Self::parse_env("MAX_DB_CONNECTIONS", "50")?,
            api_port: Self::parse_env("PORT", "8080")?,
            first_sync_mode: Self::parse_env("FIRST_SYNC_MODE", "false")?,
            auto_sync_on_startup: Self::parse_env("AUTO_SYNC_ON_STARTUP", "false")?,
            force_embedding_regen: env::var("FORCE_EMBEDDING_REGEN").ok(),
            skip_projects_sync: Self::parse_env("SKIP_PROJECTS_SYNC", "false")?,
            skip_devlogs_sync: Self::parse_env("SKIP_DEVLOGS_SYNC", "false")?,
            skip_comments_sync: Self::parse_env("SKIP_COMMENTS_SYNC", "false")?,
            skip_leaderboard_sync: Self::parse_env("SKIP_LEADERBOARD_SYNC", "false")?,
            slack_token: env::var("SLACK_TOKEN").unwrap_or_default(),
            embedding_cache_size: Self::parse_env("EMBEDDING_CACHE_SIZE", "1000")?,
            embedding_cache_ttl_seconds: Self::parse_env("EMBEDDING_CACHE_TTL_SECONDS", "3600")?,
            embedding_max_concurrent_requests: Self::parse_env("EMBEDDING_MAX_CONCURRENT_REQUESTS", "16")?,
        })
    }

    fn get_required_env(key: &str) -> Result<String> {
        env::var(key).map_err(|_| ApiError::Config(format!("{} not set", key)))
    }

    fn parse_env<T>(key: &str, default: &str) -> Result<T>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Debug,
    {
        env::var(key)
            .as_deref()
            .unwrap_or(default)
            .parse()
            .map_err(|_| ApiError::Config(format!("Invalid {}", key)))
    }
}
