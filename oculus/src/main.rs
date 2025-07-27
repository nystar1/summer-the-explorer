mod init;
mod core;
mod forge;
mod prune;
mod trace;
mod reform;
mod zenith;

use std::{collections::HashSet, sync::Arc};

use clap::{Arg, Command};
use tokio::{signal, time::Duration};

use common::{
    services::EmbeddingService,
    database::{DbPool, manager::ConnectionManager},
    utils::{config::Config, error::Result},
};

use init::InitJob;
use core::{Job, JobError, JobScheduler, progress::init_global_progress};
use forge::ForgeJob;
use prune::PruneJob;
use trace::TraceJob;
use reform::ReformJob;
use zenith::ZenithJob;

fn parse_disabled_jobs(matches: &clap::ArgMatches) -> HashSet<String> {
    let mut disabled = HashSet::with_capacity(6);

    if let Some(disabled_str) = matches.get_one::<String>("disable") {
        disabled.extend(disabled_str.split(',').map(|s| s.trim().to_owned()));
    }

    if let Ok(env_disabled) = std::env::var("DISABLE_JOBS") {
        disabled.extend(env_disabled.split(',').map(|s| s.trim().to_owned()));
    }

    disabled
}

async fn create_shared_pool(config: &Config) -> Result<Arc<DbPool>> {
    let pool = ConnectionManager::get_shared_pool(config)
        .await
        .map_err(|e| common::utils::error::ApiError::Database(e.to_string()))?;
    Ok(Arc::new(pool))
}

fn create_job(
    job_type: &str,
    config: Config,
    embedding_service: Arc<EmbeddingService>,
) -> Result<Arc<dyn Job>> {
    let job: Arc<dyn Job> = match job_type {
        "forge" => Arc::new(ForgeJob::new(config, embedding_service)),
        "prune" => Arc::new(PruneJob::new(config, embedding_service)),
        "trace" => Arc::new(TraceJob::new(config)),
        "init" => Arc::new(InitJob::new(config, embedding_service)),
        "reform" => Arc::new(ReformJob::new(config, embedding_service)),
        "zenith" => Arc::new(ZenithJob::new(config)),
        _ => {
            eprintln!(
                "Invalid job type: {}. Valid options: forge, prune, trace, init, reform, zenith",
                job_type
            );
            std::process::exit(1);
        }
    };
    Ok(job)
}

async fn run_jobs_sequential(
    job_types: &[&str],
    config: &Config,
    embedding_service: &Arc<EmbeddingService>,
) -> Result<()> {
    let shared_pool = create_shared_pool(config).await?;
    let mut scheduler = JobScheduler::new(Arc::clone(&shared_pool));
    scheduler.reserve_jobs(job_types.len());

    for job_type in job_types {
        let job = create_job(job_type, config.clone(), embedding_service.clone())?;
        scheduler.add_job(job);
    }

    tracing::info!("Running jobs: {}", job_types.join(", "));
    if let Err(e) = scheduler.run_all_sequential().await {
        tracing::error!("Job execution failed: {}", e);
        std::process::exit(1);
    }
    Ok(())
}

async fn run_single_job(
    job_type: &str,
    config: &Config, 
    embedding_service: &Arc<EmbeddingService>,
) -> Result<()> {
    let job = create_job(job_type, config.clone(), embedding_service.clone())?;
    let shared_pool = create_shared_pool(config).await?;
    let mut scheduler = JobScheduler::new(Arc::clone(&shared_pool));
    scheduler.add_job(job);
    
    tracing::info!("Running {} job", job_type);
    if let Err(e) = scheduler.run_all_sequential().await {
        tracing::error!("{} job failed: {}", job_type, e);
        std::process::exit(1);
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install crypto provider");

    let matches = Command::new("oculus")
        .about("Summer the Explorer job scheduler")
        .arg(
            Arg::new("jobs")
                .long("jobs")
                .value_name("JOB_TYPES")
                .help("Run specific jobs immediately (comma-separated: forge,prune,trace,init,reform,zenith)")
                .action(clap::ArgAction::Set)
        )
        .arg(
            Arg::new("list")
                .long("list")
                .help("List all available jobs and exit")
                .action(clap::ArgAction::SetTrue)
        )
        .arg(
            Arg::new("disable")
                .long("disable")
                .value_name("JOB_TYPES")
                .help("Disable specific jobs (comma-separated: forge,prune,trace,zenith)")
                .action(clap::ArgAction::Set)
        )
        .get_matches();

    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let env_path = std::path::Path::new(&manifest_dir).join(".env");
        if env_path.exists() {
            dotenvy::from_path(&env_path).ok();
            println!("Loaded .env file from {}", env_path.display());
        } else {
            println!("Warning: .env file not found at {}", env_path.display());
        }
    } else {
        dotenvy::dotenv().ok();
        println!("Loaded .env file from current directory or parent directories");
    }

    tracing_subscriber::fmt()
        .with_writer(std::io::stdout)
        .init();

    init_global_progress();

    if matches.get_flag("list") {
        const JOB_DESCRIPTIONS: &[(&str, &str)] = &[
            ("init", "Initialize database with fresh data"),
            ("reform", "Reform/rebuild embeddings"),
            ("forge", "Process and generate embeddings"),
            ("prune", "Clean up old/invalid data"),
            ("trace", "Continuous data monitoring"),
            ("zenith", "Peak performance optimization"),
        ];

        println!("Available jobs:");
        for (name, desc) in JOB_DESCRIPTIONS {
            println!("  {:<8} - {}", name, desc);
        }
        return Ok(());
    }

    let config = Config::from_env()?;
    let disabled_jobs = parse_disabled_jobs(&matches);

    let embedding_service = Arc::new(EmbeddingService::new(false).map_err(|e| {
        common::utils::error::ApiError::Embedding(format!(
            "Failed to create embedding service: {}",
            e
        ))
    })?);

    if let Some(job_types_str) = matches.get_one::<String>("jobs") {
        let job_types: Vec<&str> = job_types_str.split(',').map(str::trim).collect();
        run_jobs_sequential(&job_types, &config, &embedding_service).await?;
        return Ok(());
    }

    if std::env::var("RUN_REFORM")
        .unwrap_or_default()
        .to_lowercase()
        == "true"
    {
        run_single_job("reform", &config, &embedding_service).await?;
        return Ok(());
    }

    let force_wipe = std::env::var("WIPE").unwrap_or_default().to_lowercase() == "true";
    let migrate_only = std::env::var("MIGRATE_ONLY")
        .unwrap_or_default()
        .to_lowercase()
        == "true";

    async fn check_if_initialized(config: &Config) -> bool {
        if let Ok(pool) = ConnectionManager::get_shared_pool(config).await {
            if let Ok(client) = pool.get().await {
                let result = client
                    .query(
                        "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'users'",
                        &[],
                    )
                    .await;
                if let Ok(rows) = result {
                    if let Some(row) = rows.first() {
                        let count: i64 = row.get(0);
                        return count > 0;
                    }
                }
            }
        }
        false
    }

    tracing::info!("Running database migrations");
    let pool = create_shared_pool(&config).await?;
    common::database::connection::run_migrations(&pool)
        .await
        .map_err(|e| common::utils::error::ApiError::Database(e.to_string()))?;

    if migrate_only {
        tracing::info!("MIGRATE_ONLY=true detected - migrations complete, exiting");
        return Ok(());
    }

    let is_initialized = check_if_initialized(&config).await;
    let should_run_init = force_wipe || !is_initialized;

    if force_wipe {
        tracing::warn!("WIPE=true detected - will wipe database and reinitialize");
    } else if !is_initialized {
        tracing::info!("Database not initialized - running initial setup");
    }

    let shared_pool = pool.clone();

    if should_run_init {
        run_single_job("init", &config, &embedding_service).await?;
        if force_wipe {
            tracing::info!("Initialization complete - exiting due to WIPE=true");
            return Ok(());
        }
    } else {
        tracing::info!("Database already initialized - skipping init job");
    }

    tracing::info!("Starting recurring job schedulers");
    let mut handles: Vec<(
        &str,
        tokio::task::JoinHandle<std::result::Result<(), JobError>>,
    )> = Vec::with_capacity(4);

    if !disabled_jobs.contains("zenith") {
        let zenith_job = Arc::new(ZenithJob::new(config.clone()));
        let mut scheduler = JobScheduler::new(Arc::clone(&shared_pool));
        scheduler.add_job(zenith_job.clone());
        tracing::info!("Running initial zenith job at startup");
        if let Err(e) = scheduler.run_all_sequential().await {
            tracing::error!("Initial zenith job failed: {}", e);
        }

        let scheduler = JobScheduler::new(Arc::clone(&shared_pool));
        let handle = tokio::spawn(async move {
            scheduler
                .run_recurring(zenith_job, Duration::from_secs(240))
                .await
        });
        handles.push(("zenith", handle));
    } else {
        tracing::info!("Zenith job disabled");
    }

    if !disabled_jobs.contains("prune") {
        let prune_job = Arc::new(PruneJob::new(
            config.clone(),
            embedding_service.clone(),
        )) as Arc<dyn Job>;
        let scheduler = JobScheduler::new(Arc::clone(&shared_pool));
        let handle = tokio::spawn(async move {
            scheduler
                .run_recurring(prune_job, Duration::from_secs(3600))
                .await
        });
        handles.push(("prune", handle));
    } else {
        tracing::info!("Prune job disabled");
    }

    if !disabled_jobs.contains("forge") {
        let forge_job = Arc::new(ForgeJob::new(
            config.clone(),
            embedding_service.clone(),
        )) as Arc<dyn Job>;
        let scheduler = JobScheduler::new(Arc::clone(&shared_pool));
        let handle = tokio::spawn(async move {
            scheduler
                .run_recurring(forge_job, Duration::from_secs(120))
                .await
        });
        handles.push(("forge", handle));
    } else {
        tracing::info!("Forge job disabled");
    }

    if !disabled_jobs.contains("trace") {
        let trace_job = Arc::new(TraceJob::new(config.clone())) as Arc<dyn Job>;
        let scheduler = JobScheduler::new(Arc::clone(&shared_pool));
        let handle = tokio::spawn(async move {
            scheduler
                .run_continuous(trace_job, Duration::from_secs(120))
                .await
        });
        handles.push(("trace", handle));
    } else {
        tracing::info!("Trace job disabled");
    }

    if handles.is_empty() {
        tracing::warn!("All jobs are disabled, exiting");
        return Ok(());
    }

    tracing::info!("All job schedulers started. Waiting for shutdown signal...");

    signal::ctrl_c().await?;

    tracing::info!("Shutdown signal received, terminating jobs...");

    for (job_name, handle) in handles {
        tracing::info!("Terminating {} job", job_name);
        handle.abort();
    }

    Ok(())
}
