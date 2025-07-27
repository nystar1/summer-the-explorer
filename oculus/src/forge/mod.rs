mod sync;
mod fetch;
mod store;

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Semaphore;
use futures::stream::{FuturesUnordered, StreamExt};

use common::{
    database::DbPool,
    utils::config::Config,
    services::{EmbeddingService, external::ExternalApiService},
};

use crate::core::{Job, JobError, get_embedding_concurrency, progress::get_job_progress, progress::create_embedding_progress};

use fetch::DataFetcher;
use store::DataStore;
use sync::DataSyncer;

pub struct ForgeJob {
    config: Config,
    embedding_service: Arc<EmbeddingService>,
}

impl ForgeJob {
    pub fn new(config: Config, embedding_service: Arc<EmbeddingService>) -> Self {
        Self {
            config,
            embedding_service,
        }
    }

    async fn store_projects_with_parallel_embeddings(
        &self,
        projects: Vec<common::utils::modal::RawProject>,
        pool: &DbPool,
    ) -> Result<(), JobError> {
        if projects.is_empty() {
            return Ok(());
        }

        let embedding_progress = create_embedding_progress("forge", "projects");
        embedding_progress.init(projects.len());

        let concurrency = get_embedding_concurrency();
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let embedding_service = Arc::clone(&self.embedding_service);
        let pool = Arc::new(pool.clone());

        let mut futures = FuturesUnordered::new();

        for project in projects {
            let semaphore = Arc::clone(&semaphore);
            let embedding_service = Arc::clone(&embedding_service);
            let pool = Arc::clone(&pool);
            let embedding_progress = embedding_progress.clone();

            let future = async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|e| JobError::Embedding(format!("Semaphore error: {}", e)))?;

                let result = DataStore::store_project_with_embedding(&project, &embedding_service, &pool).await;
                embedding_progress.increment();
                result
            };

            futures.push(future);
        }

        while let Some(result) = futures.next().await {
            if let Err(_e) = result {
            }
        }

        embedding_progress.done("All projects processed".to_string());
        Ok(())
    }

    async fn store_comments_with_parallel_embeddings(
        &self,
        comments: Vec<common::utils::modal::RawComment>,
        pool: &DbPool,
    ) -> Result<(), JobError> {
        if comments.is_empty() {
            return Ok(());
        }

        let embedding_progress = create_embedding_progress("forge", "comments");
        embedding_progress.init(comments.len());

        let concurrency = get_embedding_concurrency();
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let embedding_service = Arc::clone(&self.embedding_service);
        let pool = Arc::new(pool.clone());

        let mut futures = FuturesUnordered::new();

        for comment in comments {
            let semaphore = Arc::clone(&semaphore);
            let embedding_service = Arc::clone(&embedding_service);
            let pool = Arc::clone(&pool);
            let embedding_progress = embedding_progress.clone();

            let future = async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|e| JobError::Embedding(format!("Semaphore error: {}", e)))?;

                let result = DataStore::store_comment_with_embedding(&comment, &embedding_service, &pool).await;
                embedding_progress.increment();
                result
            };

            futures.push(future);
        }

        while let Some(result) = futures.next().await {
            if let Err(_e) = result {
            }
        }

        embedding_progress.done("All comments processed".to_string());
        Ok(())
    }

    async fn store_devlogs_with_parallel_embeddings(
        &self,
        devlogs: Vec<common::utils::modal::RawDevlog>,
        pool: &DbPool,
    ) -> Result<(), JobError> {
        if devlogs.is_empty() {
            return Ok(());
        }

        let embedding_progress = create_embedding_progress("forge", "devlogs");
        embedding_progress.init(devlogs.len());

        let concurrency = get_embedding_concurrency();
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let embedding_service = Arc::clone(&self.embedding_service);
        let pool = Arc::new(pool.clone());

        let mut futures = FuturesUnordered::new();

        for devlog in devlogs {
            let semaphore = Arc::clone(&semaphore);
            let embedding_service = Arc::clone(&embedding_service);
            let pool = Arc::clone(&pool);
            let embedding_progress = embedding_progress.clone();

            let future = async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|e| JobError::Embedding(format!("Semaphore error: {}", e)))?;

                let result = DataStore::store_devlog_with_embedding(&devlog, &embedding_service, &pool).await;
                embedding_progress.increment();
                result
            };

            futures.push(future);
        }

        while let Some(result) = futures.next().await {
            if let Err(_e) = result {
            }
        }

        embedding_progress.done("All devlogs processed".to_string());
        Ok(())
    }

    async fn store_with_parallel_embeddings(
        &self,
        projects: Vec<common::utils::modal::RawProject>,
        comments: Vec<common::utils::modal::RawComment>,
        devlogs: Vec<common::utils::modal::RawDevlog>,
        pool: &DbPool,
    ) -> Result<(), JobError> {
        self.store_projects_with_parallel_embeddings(projects, pool).await?;
        self.store_comments_with_parallel_embeddings(comments, pool).await?;
        self.store_devlogs_with_parallel_embeddings(devlogs, pool).await?;
        Ok(())
    }
}

#[async_trait]
impl Job for ForgeJob {
    async fn execute(&self, pool: &DbPool) -> Result<(), JobError> {
        let pool = Arc::new(pool.clone());

        let external_api = Arc::new(
            ExternalApiService::new(self.config.journey_session_cookie.clone())
                .map_err(|e| JobError::ExternalApi(e.to_string()))?,
        );

        let progress = get_job_progress("forge");
        progress.update_progress(0, 3, "Fetching new projects");

        let (new_projects, projects_last_page) =
            DataFetcher::fetch_new_projects(&external_api, &pool).await?;

        progress.update_progress(1, 3, "Fetching new comments");
        let comments_meta = DataSyncer::get_last_sync_metadata(&pool, "comments").await?;
        let (new_comments, comments_last_page) =
            DataFetcher::fetch_new_comments(&external_api, comments_meta.map(|(_, p)| p)).await?;

        progress.update_progress(2, 3, "Fetching new devlogs");
        let devlogs_meta = DataSyncer::get_last_sync_metadata(&pool, "devlogs").await?;
        let (new_devlogs, devlogs_last_page) =
            DataFetcher::fetch_new_devlogs(&external_api, devlogs_meta.map(|(_, p)| p)).await?;

        progress.update_progress(
            3,
            3,
            &format!(
                "Found {} new projects, {} new comments, {} new devlogs",
                new_projects.len(),
                new_comments.len(),
                new_devlogs.len()
            ),
        );

        if !new_projects.is_empty() || !new_comments.is_empty() || !new_devlogs.is_empty() {
            self.store_with_parallel_embeddings(new_projects, new_comments, new_devlogs, &pool)
                .await?;

            if projects_last_page > 0 {
                DataSyncer::update_sync_metadata(&pool, "projects", projects_last_page).await?;
            }
            if comments_last_page > 0 {
                DataSyncer::update_sync_metadata(&pool, "comments", comments_last_page).await?;
            }
            if devlogs_last_page > 0 {
                DataSyncer::update_sync_metadata(&pool, "devlogs", devlogs_last_page).await?;
            }
        }

        DataSyncer::sync_user_shell_data(&external_api, &pool).await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "ForgeJob"
    }
}