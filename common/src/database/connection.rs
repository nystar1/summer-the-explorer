use std::{path::Path, time::Duration};

use tracing::{error, info};
use tokio_postgres_rustls::MakeRustlsConnect;
use deadpool_postgres::{
    Config as PoolConfig, ManagerConfig, Pool, RecyclingMethod, Runtime, Timeouts,
};

use crate::utils::{
    config::Config,
    certs::GLOBAL_BUNDLE_PEM,
    error::{ApiError, Result},
};

pub type DbPool = Pool;

pub async fn create_pool(config: &Config) -> Result<DbPool> {
    let mut cfg = PoolConfig::new();
    cfg.url = Some(config.database_url.clone());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Verified,
    });

    cfg.pool = Some(deadpool_postgres::PoolConfig {
        max_size: config.max_db_connections as usize,
        timeouts: Timeouts {
            wait: Some(Duration::from_secs(10)),
            create: Some(Duration::from_secs(5)),
            recycle: Some(Duration::from_secs(10)),
        },
        ..Default::default()
    });

    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut cert_reader = GLOBAL_BUNDLE_PEM;
    for cert in rustls_pemfile::certs(&mut cert_reader).flatten() {
        let _ = root_store.add(cert);
    }

    let tls_connector = MakeRustlsConnect::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    );

    let pool = cfg
        .create_pool(Some(Runtime::Tokio1), tls_connector.clone())
        .map_err(|e| ApiError::Database(format!("Failed to create database pool: {}", e)))?;

    let _ = pool.get()
        .await
        .map_err(|e| ApiError::Database(format!("Database connection test failed: {}", e)))?;

    Ok(pool)
}

pub async fn run_migrations(pool: &DbPool) -> Result<()> {
    const MIGRATION_PATHS: [&str; 3] = ["../migrations", "./migrations", "migrations"];

    let client = pool
        .get()
        .await
        .map_err(|e| ApiError::Database(format!("Failed to get client: {e}")))?;

    client
        .execute(
            "CREATE TABLE IF NOT EXISTS __migrations (
            filename TEXT PRIMARY KEY,
            applied_at TIMESTAMPTZ DEFAULT NOW()
        )",
            &[],
        )
        .await
        .map_err(|e| ApiError::Database(format!("Failed to create migrations table: {e}")))?;

    let migration_dir = MIGRATION_PATHS
        .iter()
        .map(Path::new)
        .find(|path| path.exists())
        .ok_or_else(|| ApiError::Database("No migrations directory found".to_owned()))?;
    let mut migrations = std::fs::read_dir(migration_dir)
        .map_err(|e| ApiError::Database(format!("Failed to read migrations directory: {e}")))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()? == "sql" {
                Some(path)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    migrations.sort();

    for migration_path in migrations {
        let migration_name = migration_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| ApiError::Database("Invalid migration filename".to_owned()))?;

        let already_applied = client
            .query_opt(
                "SELECT 1 FROM __migrations WHERE filename = $1",
                &[&migration_name],
            )
            .await
            .map_err(|e| ApiError::Database(format!("Failed to check migration status: {e}")))?;

        if already_applied.is_some() {
            info!("Skipping already applied migration: {}", migration_name);
            continue;
        }

        let migration_sql = std::fs::read_to_string(&migration_path).map_err(|e| {
            ApiError::Database(format!(
                "Failed to read migration {migration_name}: {e}"
            ))
        })?;

        info!("Running migration: {}", migration_name);

        match client.batch_execute(&migration_sql).await {
            Ok(()) => {
                client
                    .execute(
                        "INSERT INTO __migrations (filename) VALUES ($1)",
                        &[&migration_name],
                    )
                    .await
                    .map_err(|e| {
                        ApiError::Database(format!("Failed to mark migration as applied: {e}"))
                    })?;

                info!("Successfully applied migration: {}", migration_name);
            }
            Err(e) => {
                error!("Migration {} failed: {}", migration_name, e);
                return Err(ApiError::Database(format!(
                    "Migration {migration_name} failed: {e}"
                )));
            }
        }
    }

    info!("All migrations completed successfully");
    Ok(())
}
