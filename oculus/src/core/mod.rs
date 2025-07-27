use std::sync::Arc;

use dashmap::DashMap;
use async_trait::async_trait;
use tokio::{
    sync::Mutex as AsyncMutex,
    time::{sleep, Duration},
};

use common::database::DbPool;

pub mod progress;

const MAX_JOB_TYPES: usize = 6;
const MAX_RETRIES: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_secs(30);

pub fn get_base_concurrency() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

pub fn get_embedding_concurrency() -> usize {
    get_base_concurrency() * 2
}

pub fn get_fetch_concurrency() -> usize {
    (get_base_concurrency() * 4).min(20)
}

pub async fn with_retry<T, F, Fut>(operation_name: &str, operation: F) -> Result<T, JobError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, common::utils::error::ApiError>>,
{
    for attempt in 1..=MAX_RETRIES {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if attempt == MAX_RETRIES {
                    return Err(JobError::ExternalApi(format!(
                        "Failed {} after {} retries: {}",
                        operation_name, MAX_RETRIES, e
                    )));
                }
                let delay = Duration::from_millis(1000 * attempt as u64);
                tracing::warn!(
                    "Attempt {}/{} failed for {}: {}. Retrying in {:?}...",
                    attempt, MAX_RETRIES, operation_name, e, delay
                );
                sleep(delay).await;
            }
        }
    }
    unreachable!()
}


pub fn parse_datetime(datetime_str: &str) -> Result<chrono::DateTime<chrono::Utc>, JobError> {
    chrono::DateTime::parse_from_rfc3339(datetime_str)
        .map_err(|e| JobError::Database(format!("Invalid datetime format: {}", e)))
        .map(|dt| dt.with_timezone(&chrono::Utc))
}


#[async_trait]
pub trait Job: Send + Sync + 'static {
    async fn execute(&self, pool: &DbPool) -> Result<(), JobError>;
    fn name(&self) -> &str;
}

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("External API error: {0}")]
    ExternalApi(String),
    #[error("Embedding error: {0}")]
    Embedding(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Other error: {0}")]
    Other(String),
}

pub struct JobScheduler {
    jobs: Vec<Arc<dyn Job>>,
    job_locks: Arc<DashMap<String, Arc<AsyncMutex<()>>>>,
    pool: Arc<DbPool>,
}

impl JobScheduler {
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self {
            jobs: Vec::with_capacity(MAX_JOB_TYPES),
            job_locks: Arc::new(DashMap::with_capacity(MAX_JOB_TYPES)),
            pool,
        }
    }

    async fn get_job_lock(&self, job_name: &str) -> Arc<AsyncMutex<()>> {
        self.job_locks
            .entry(job_name.to_owned())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    pub fn reserve_jobs(&mut self, additional: usize) {
        self.jobs.reserve(additional);
    }

    pub fn add_job(&mut self, job: Arc<dyn Job>) {
        self.jobs.push(job);
    }

    pub async fn run_all_sequential(&self) -> Result<(), JobError> {
        for job in &self.jobs {
            tracing::info!("Starting job: {}", job.name());
            job.execute(&self.pool).await?;
            tracing::info!("Completed job: {}", job.name());
        }
        Ok(())
    }

    pub async fn run_recurring(
        &self,
        job: Arc<dyn Job>,
        interval: Duration,
    ) -> Result<(), JobError> {
        loop {
            let job_lock = self.get_job_lock(job.name()).await;
            let _guard = job_lock.lock().await;

            tracing::info!("Starting recurring job: {}", job.name());

            let mut attempts = 0;

            loop {
                attempts += 1;
                match job.execute(&self.pool).await {
                    Ok(()) => {
                        tracing::info!("Completed recurring job: {}", job.name());
                        break;
                    }
                    Err(e) => {
                        if attempts < MAX_RETRIES {
                            tracing::warn!(
                                "Failed recurring job {} (attempt {}/{}): {}. Retrying in {:?}",
                                job.name(),
                                attempts,
                                MAX_RETRIES,
                                e,
                                RETRY_DELAY
                            );
                            sleep(RETRY_DELAY).await;
                        } else {
                            tracing::error!(
                                "Failed recurring job {} after {} attempts: {}",
                                job.name(),
                                MAX_RETRIES,
                                e
                            );
                            break;
                        }
                    }
                }
            }

            sleep(interval).await;
        }
    }

    pub async fn run_continuous(
        &self,
        job: Arc<dyn Job>,
        check_interval: Duration,
    ) -> Result<(), JobError> {
        loop {
            let job_lock = self.get_job_lock(job.name()).await;
            let _guard = job_lock.lock().await;

            tracing::info!("Checking for work in continuous job: {}", job.name());
            match job.execute(&self.pool).await {
                Ok(()) => continue,
                Err(JobError::Other(ref msg)) if msg == "no_work" => {
                    tracing::debug!(
                        "No work available for {}, sleeping for {:?}",
                        job.name(),
                        check_interval
                    );
                    sleep(check_interval).await;
                }
                Err(e) => {
                    tracing::error!("Error in continuous job {}: {}", job.name(), e);
                    sleep(check_interval).await;
                }
            }
        }
    }
}
