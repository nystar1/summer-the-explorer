use crate::core::progress::ProgressReporter;
use crate::core::{with_retry, Job, JobError};
use async_trait::async_trait;
use common::{
    database::manager::ConnectionManager,
    services::{external::ExternalApiService, EmbeddingService},
    utils::config::Config,
};
use std::sync::Arc;


pub struct PruneJob {
    config: Config,
    embedding_service: Arc<EmbeddingService>,
}

impl PruneJob {
    pub fn new(config: Config, embedding_service: Arc<EmbeddingService>) -> Self {
        Self {
            config,
            embedding_service,
        }
    }

    async fn cleanup_orphaned_data(&self, pool: &common::database::DbPool) -> Result<(), JobError> {
        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let cleanup_queries = [
            "DELETE FROM comments WHERE devlog_id NOT IN (SELECT id FROM logs)",
            "DELETE FROM logs WHERE project_id NOT IN (SELECT id FROM projects)",
            "DELETE FROM shell_history WHERE slack_id NOT IN (SELECT slack_id FROM users)",
        ];

        for query in &cleanup_queries {
            client
                .execute(*query, &[])
                .await
                .map_err(|e| JobError::Database(e.to_string()))?;
        }

        Ok(())
    }

    async fn fetch_all_external_projects(
        &self,
        external_api: &ExternalApiService,
    ) -> Result<std::collections::HashMap<i64, common::utils::modal::RawProject>, JobError> {
        let mut all_projects = std::collections::HashMap::new();
        let mut page = 1;

        loop {
            let response = with_retry(&format!("fetch_external_projects_page_{}", page), || {
                external_api.fetch_projects(Some(page))
            })
            .await
            .map_err(|e| JobError::ExternalApi(e.to_string()))?;

            if response.projects.is_empty() {
                break;
            }

            for project in response.projects {
                all_projects.insert(project.id, project);
            }

            if let Some(pagination) = response.pagination {
                if let Some(total_pages) = pagination.pages {
                    if page >= total_pages {
                        break;
                    }
                }
            }

            page += 1;
        }

        Ok(all_projects)
    }

    async fn fetch_all_external_devlogs(
        &self,
        external_api: &ExternalApiService,
    ) -> Result<std::collections::HashMap<i64, common::utils::modal::RawDevlog>, JobError> {
        let mut all_devlogs = std::collections::HashMap::new();
        let mut page = 1;

        loop {
            let response = with_retry(&format!("fetch_external_devlogs_page_{}", page), || {
                external_api.fetch_devlogs(Some(page))
            })
            .await
            .map_err(|e| JobError::ExternalApi(e.to_string()))?;

            if response.devlogs.is_empty() {
                break;
            }

            for devlog in response.devlogs {
                all_devlogs.insert(devlog.id, devlog);
            }

            if let Some(pagination) = response.pagination {
                if let Some(total_pages) = pagination.pages {
                    if page >= total_pages {
                        break;
                    }
                }
            }

            page += 1;
        }

        Ok(all_devlogs)
    }

    async fn prune_and_update_projects(
        &self,
        external_projects: &std::collections::HashMap<i64, common::utils::modal::RawProject>,
        embedding_service: &EmbeddingService,
        pool: &common::database::DbPool,
    ) -> Result<(), JobError> {
        let mut client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let db_items = client
            .query("SELECT id, title, description, updated_at FROM projects", &[])
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let total_items = db_items.len();
        let progress = ProgressReporter::new_with_job("prune", "Pruning and updating projects");

        for (i, row) in db_items.iter().enumerate() {
            progress.report(i + 1, total_items);
            let item_id: i64 = row.get(0);
            let db_title: String = row.get(1);
            let db_description: Option<String> = row.get(2);
            let db_updated_at: chrono::DateTime<chrono::Utc> = row.get(3);

            if let Some(external_project) = external_projects.get(&item_id) {
                let external_content = format!("{} {}", external_project.title, external_project.description.as_deref().unwrap_or_default()).trim().to_string();
                let db_content = format!("{} {}", db_title, db_description.as_deref().unwrap_or_default()).trim().to_string();
                
                let external_updated_at = crate::core::parse_datetime(&external_project.updated_at)?;
                let needs_update = external_updated_at > db_updated_at || db_content != external_content;

                if needs_update {
                    let embedding_vec = embedding_service
                        .embed_text(&external_content)
                        .await
                        .map_err(|e| JobError::Embedding(e.to_string()))?;

                    let embedding = pgvector::Vector::from(embedding_vec);

                    client.execute(
                        "UPDATE projects SET title = $1, description = $2, updated_at = $3, title_description_embedding = $4 WHERE id = $5",
                        &[&external_project.title, &external_project.description, &external_updated_at, &embedding, &item_id]
                    ).await
                    .map_err(|e| JobError::Database(e.to_string()))?;
                }
            } else {
                let tx_client = client
                    .transaction()
                    .await
                    .map_err(|e| JobError::Database(e.to_string()))?;

                tx_client
                    .execute("DELETE FROM projects WHERE id = $1", &[&item_id])
                    .await
                    .map_err(|e| JobError::Database(e.to_string()))?;

                tx_client
                    .commit()
                    .await
                    .map_err(|e| JobError::Database(e.to_string()))?;
            }
        }

        progress.finish();
        Ok(())
    }

    async fn prune_and_update_devlogs(
        &self,
        external_devlogs: &std::collections::HashMap<i64, common::utils::modal::RawDevlog>,
        embedding_service: &EmbeddingService,
        pool: &common::database::DbPool,
    ) -> Result<(), JobError> {
        let mut client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let db_items = client
            .query("SELECT id, text, updated_at FROM logs", &[])
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let total_items = db_items.len();
        let progress = ProgressReporter::new_with_job("prune", "Pruning and updating devlogs");

        for (i, row) in db_items.iter().enumerate() {
            progress.report(i + 1, total_items);
            let item_id: i64 = row.get(0);
            let db_content: String = row.get(1);
            let db_updated_at: chrono::DateTime<chrono::Utc> = row.get(2);

            if let Some(external_devlog) = external_devlogs.get(&item_id) {
                let external_content = external_devlog.text.clone();
                let external_updated_at = crate::core::parse_datetime(&external_devlog.updated_at)?;
                let needs_update = external_updated_at > db_updated_at || db_content != external_content;

                if needs_update {
                    let embedding_vec = embedding_service
                        .embed_text(&external_content)
                        .await
                        .map_err(|e| JobError::Embedding(e.to_string()))?;

                    let embedding = pgvector::Vector::from(embedding_vec);

                    client.execute(
                        "UPDATE logs SET text = $1, updated_at = $2, text_embedding = $3 WHERE id = $4",
                        &[&external_content, &external_updated_at, &embedding, &item_id]
                    ).await
                    .map_err(|e| JobError::Database(e.to_string()))?;
                }
            } else {
                let tx_client = client
                    .transaction()
                    .await
                    .map_err(|e| JobError::Database(e.to_string()))?;

                tx_client
                    .execute("DELETE FROM logs WHERE id = $1", &[&item_id])
                    .await
                    .map_err(|e| JobError::Database(e.to_string()))?;

                tx_client
                    .commit()
                    .await
                    .map_err(|e| JobError::Database(e.to_string()))?;
            }
        }

        progress.finish();
        Ok(())
    }
}

#[async_trait]
impl Job for PruneJob {
    async fn execute(&self, _: &common::database::DbPool) -> Result<(), JobError> {
        let pool = Arc::new(
            ConnectionManager::get_dedicated_pool(&self.config)
                .await
                .map_err(|e| JobError::Database(e.to_string()))?,
        );

        let external_api = Arc::new(
            ExternalApiService::new(self.config.journey_session_cookie.clone())
                .map_err(|e| JobError::ExternalApi(e.to_string()))?,
        );

        let external_projects = self.fetch_all_external_projects(&external_api).await?;

        let external_devlogs = self.fetch_all_external_devlogs(&external_api).await?;

        self.prune_and_update_projects(
            &external_projects,
            &self.embedding_service,
            &pool,
        ).await?;

        self.prune_and_update_devlogs(
            &external_devlogs,
            &self.embedding_service,
            &pool,
        ).await?;

        self.cleanup_orphaned_data(&pool).await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "PruneJob"
    }
}