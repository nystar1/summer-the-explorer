use crate::core::{get_fetch_concurrency, progress::create_progress_with_job, JobError};
use common::{
    database::connection,
    services::external::ExternalApiService,
    utils::modal::{RawComment, RawDevlog, RawProject},
};
use futures::stream::{FuturesUnordered, StreamExt};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub struct DataFetcher;

#[derive(Debug)]
pub enum DataType {
    Projects,
    Comments,
    Devlogs,
}

impl DataType {
    fn capacity_hint(&self) -> usize {
        match self {
            Self::Projects => 100,
            Self::Comments => 200,
            Self::Devlogs => 500,
        }
    }

    fn progress_name(&self) -> &'static str {
        match self {
            Self::Projects => "Fetching new projects",
            Self::Comments => "Fetching new comments", 
            Self::Devlogs => "Fetching new devlogs",
        }
    }
}

async fn fetch_with_concurrency<T, F, Fut>(
    data_type: DataType,
    start_page: i32,
    total_pages: i32,
    fetcher: F,
    _existing_filter: Option<Arc<HashSet<i64>>>,
) -> Result<(Vec<T>, i32), JobError>
where
    T: Send + 'static,
    F: Fn(i32) -> Fut + Send + Sync + Clone,
    Fut: std::future::Future<Output = Result<Vec<T>, JobError>> + Send,
{
    let concurrency_limit = std::env::var("FETCH_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or_else(get_fetch_concurrency);

    let semaphore = Arc::new(Semaphore::new(concurrency_limit));
    let mut futures = FuturesUnordered::new();
    let progress = create_progress_with_job("forge", data_type.progress_name());
    progress.init(Some((total_pages - start_page + 1) as usize), Some("pages"));

    for page in start_page..=total_pages {
        let semaphore = semaphore.clone();
        let fetcher = fetcher.clone();

        let future = async move {
            let _permit = semaphore
                .acquire()
                .await
                .map_err(|e| JobError::ExternalApi(format!("Semaphore error: {}", e)))?;

            let items = fetcher(page).await?;
            
            Result::<(i32, Vec<T>), JobError>::Ok((page, items))
        };

        futures.push(future);
    }

    let mut all_items = Vec::with_capacity(data_type.capacity_hint());
    let mut current_page = start_page;
    let mut pages_processed = 0;

    while let Some(result) = futures.next().await {
        match result {
            Ok((page, items)) => {
                all_items.extend(items);
                pages_processed += 1;
                progress.set(pages_processed);
                current_page = current_page.max(page);
            }
            Err(e) => {
                tracing::warn!("Failed to fetch {} page: {}", 
                    match data_type {
                        DataType::Projects => "projects",
                        DataType::Comments => "comments", 
                        DataType::Devlogs => "devlogs",
                    }, e);
                continue;
            }
        }
    }

    progress.done(format!("Found {} new {}", all_items.len(), 
        match data_type {
            DataType::Projects => "projects",
            DataType::Comments => "comments",
            DataType::Devlogs => "devlogs", 
        }));
    
    Ok((all_items, current_page))
}

impl DataFetcher {
    pub async fn fetch_new_projects(
        external_api: &ExternalApiService,
        pool: &connection::DbPool,
    ) -> Result<(Vec<RawProject>, i32), JobError> {
        let start_page = super::sync::DataSyncer::calculate_start_page(pool).await?;

        tracing::info!(
            "Starting project fetch from page {} (calculated from existing project count)",
            start_page
        );

        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let existing_rows = client
            .query("SELECT id FROM projects", &[])
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let existing_ids: HashSet<i64> = existing_rows
            .iter()
            .map(|row| row.get::<_, i64>("id"))
            .collect();

        tracing::info!(
            "Loaded {} existing project IDs for duplicate checking",
            existing_ids.len()
        );

        let first_response = external_api
            .fetch_projects(Some(start_page))
            .await
            .map_err(|e| JobError::ExternalApi(e.to_string()))?;

        if first_response.projects.is_empty() {
            return Ok((Vec::new(), start_page - 1));
        }

        let total_pages = first_response.pagination.and_then(|p| p.pages).unwrap_or(start_page);
        
        let mut new_projects: Vec<RawProject> = first_response.projects
            .into_iter()
            .filter(|project| !existing_ids.contains(&project.id))
            .collect();

        if start_page >= total_pages {
            return Ok((new_projects, start_page));
        }

        let existing_ids = Arc::new(existing_ids);
        let existing_ids_clone = existing_ids.clone();
        let external_api_clone = external_api.clone();
        let (additional_projects, last_page) = fetch_with_concurrency(
            DataType::Projects,
            start_page + 1,
            total_pages,
            move |page| {
                let existing_ids = existing_ids_clone.clone();
                let external_api = external_api_clone.clone();
                async move {
                    let response = external_api
                        .fetch_projects(Some(page))
                        .await
                        .map_err(|e| JobError::ExternalApi(e.to_string()))?;

                    let filtered_projects: Vec<RawProject> = response.projects
                        .into_iter()
                        .filter(|project| !existing_ids.contains(&project.id))
                        .collect();

                    Ok(filtered_projects)
                }
            },
            Some(existing_ids),
        ).await?;

        new_projects.extend(additional_projects);
        Ok((new_projects, last_page.max(start_page)))
    }

    pub async fn fetch_new_comments(
        external_api: &ExternalApiService,
        last_page: Option<i32>,
    ) -> Result<(Vec<RawComment>, i32), JobError> {
        let start_page = last_page.map(|p| p + 1).unwrap_or(1);

        tracing::info!("Starting comment fetch from page {}", start_page);

        let first_response = external_api
            .fetch_comments(Some(start_page))
            .await
            .map_err(|e| JobError::ExternalApi(e.to_string()))?;

        if first_response.comments.is_empty() {
            return Ok((Vec::new(), start_page - 1));
        }

        let total_pages = first_response.pagination
            .and_then(|p| p.pages)
            .unwrap_or(start_page);

        let mut all_comments = first_response.comments;

        if start_page >= total_pages {
            return Ok((all_comments, start_page));
        }

        let external_api_clone = external_api.clone();
        let (additional_comments, last_page) = fetch_with_concurrency(
            DataType::Comments,
            start_page + 1,
            total_pages,
            move |page| {
                let external_api = external_api_clone.clone();
                async move {
                    let response = external_api
                        .fetch_comments(Some(page))
                        .await
                        .map_err(|e| JobError::ExternalApi(e.to_string()))?;

                    Ok(response.comments)
                }
            },
            None,
        ).await?;

        all_comments.extend(additional_comments);
        Ok((all_comments, last_page.max(start_page)))
    }

    pub async fn fetch_new_devlogs(
        external_api: &ExternalApiService,
        last_page: Option<i32>,
    ) -> Result<(Vec<RawDevlog>, i32), JobError> {
        let start_page = last_page.map(|p| p + 1).unwrap_or(1);

        tracing::info!("Starting devlog fetch from page {}", start_page);

        let first_response = external_api
            .fetch_devlogs(Some(start_page))
            .await
            .map_err(|e| JobError::ExternalApi(e.to_string()))?;

        if first_response.devlogs.is_empty() {
            return Ok((Vec::new(), start_page - 1));
        }

        let total_pages = first_response.pagination
            .and_then(|p| p.pages)
            .unwrap_or(start_page);

        let mut all_devlogs = first_response.devlogs;

        if start_page >= total_pages {
            return Ok((all_devlogs, start_page));
        }

        let external_api_clone = external_api.clone();
        let (additional_devlogs, last_page) = fetch_with_concurrency(
            DataType::Devlogs,
            start_page + 1,
            total_pages,
            move |page| {
                let external_api = external_api_clone.clone();
                async move {
                    let response = external_api
                        .fetch_devlogs(Some(page))
                        .await
                        .map_err(|e| JobError::ExternalApi(e.to_string()))?;

                    Ok(response.devlogs)
                }
            },
            None,
        ).await?;

        all_devlogs.extend(additional_devlogs);
        Ok((all_devlogs, last_page.max(start_page)))
    }
}