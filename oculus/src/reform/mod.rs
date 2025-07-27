use crate::core::progress::ProgressReporter;
use crate::core::{Job, JobError};
use async_trait::async_trait;
use common::{
    database::connection::{create_pool, run_migrations},
    services::EmbeddingService,
    utils::config::Config,
    DbPool,
};
use std::sync::Arc;

#[derive(Debug, Clone)]
enum Target {
    Projects,
    Comments,
    Devlogs,
    All,
}

fn get_target_from_env() -> Target {
    match std::env::var("REEMBED_TARGET")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "projects" => Target::Projects,
        "comments" => Target::Comments,
        "devlogs" => Target::Devlogs,
        _ => Target::All,
    }
}

pub struct ReformJob {
    config: Config,
    embedding_service: Arc<EmbeddingService>,
}

impl ReformJob {
    pub fn new(config: Config, embedding_service: Arc<EmbeddingService>) -> Self {
        Self {
            config,
            embedding_service,
        }
    }

    async fn embed_projects_from_db(
        &self,
        embedding: &EmbeddingService,
        pool: &common::database::connection::DbPool,
    ) -> Result<(), JobError> {
        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;
        let rows = client
            .query("SELECT id, title, description FROM projects", &[])
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;
        let total = rows.len();
        let progress_reporter = ProgressReporter::new_with_job("reform", "Re-embedding projects");
        for (i, row) in rows.iter().enumerate() {
            progress_reporter.report(i + 1, total);
            let id: i64 = row.get("id");
            let title: String = row.get("title");
            let description: Option<String> = row.get("description");
            let text = format!("{} {}", title, description.unwrap_or_default());
            let vec = embedding
                .embed_text(&text)
                .await
                .map_err(|e| JobError::Embedding(e.to_string()))?;
            let vector = pgvector::Vector::from(vec);
            client
                .execute(
                    "UPDATE projects SET title_description_embedding = $2 WHERE id = $1",
                    &[&id, &vector],
                )
                .await
                .map_err(|e| JobError::Database(e.to_string()))?;
        }
        progress_reporter.finish();
        Ok(())
    }

    async fn embed_comments_from_db(
        &self,
        embedding: &EmbeddingService,
        pool: &common::database::connection::DbPool,
    ) -> Result<(), JobError> {
        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;
        let rows = client
            .query("SELECT devlog_id, slack_id, text FROM comments", &[])
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;
        let total = rows.len();
        let progress_reporter = ProgressReporter::new_with_job("reform", "Re-embedding comments");
        for (i, row) in rows.iter().enumerate() {
            progress_reporter.report(i + 1, total);
            let devlog_id: i64 = row.get("devlog_id");
            let slack_id: String = row.get("slack_id");
            let text: String = row.get("text");
            let vec = embedding
                .embed_text(&text)
                .await
                .map_err(|e| JobError::Embedding(e.to_string()))?;
            let vector = pgvector::Vector::from(vec);
            client
                .execute("UPDATE comments SET text_embedding = $3 WHERE devlog_id = $1 AND slack_id = $2", &[&devlog_id, &slack_id, &vector])
                .await
                .map_err(|e| JobError::Database(e.to_string()))?;
        }
        progress_reporter.finish();
        Ok(())
    }

    async fn embed_devlogs_from_db(
        &self,
        embedding: &EmbeddingService,
        pool: &common::database::connection::DbPool,
    ) -> Result<(), JobError> {
        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;
        let rows = client
            .query("SELECT id, text FROM logs", &[])
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;
        let total = rows.len();
        let progress_reporter = ProgressReporter::new_with_job("reform", "Re-embedding devlogs");
        for (i, row) in rows.iter().enumerate() {
            progress_reporter.report(i + 1, total);
            let id: i64 = row.get("id");
            let text: String = row.get("text");
            let vec = embedding
                .embed_text(&text)
                .await
                .map_err(|e| JobError::Embedding(e.to_string()))?;
            let vector = pgvector::Vector::from(vec);
            client
                .execute(
                    "UPDATE logs SET text_embedding = $2 WHERE id = $1",
                    &[&id, &vector],
                )
                .await
                .map_err(|e| JobError::Database(e.to_string()))?;
        }
        progress_reporter.finish();
        Ok(())
    }
}

#[async_trait]
impl Job for ReformJob {
    async fn execute(&self, _pool: &DbPool) -> Result<(), JobError> {
        tracing::info!("Starting reform embedding job");
        let pool = Arc::new(
            create_pool(&self.config)
                .await
                .map_err(|e| JobError::Database(e.to_string()))?,
        );
        run_migrations(&pool)
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let embedding_service = &self.embedding_service;
        let target = get_target_from_env();

        match target {
            Target::Projects => {
                self.embed_projects_from_db(embedding_service, &pool)
                    .await?
            }
            Target::Comments => {
                self.embed_comments_from_db(embedding_service, &pool)
                    .await?
            }
            Target::Devlogs => self.embed_devlogs_from_db(embedding_service, &pool).await?,
            Target::All => {
                self.embed_projects_from_db(embedding_service, &pool)
                    .await?;
                self.embed_comments_from_db(embedding_service, &pool)
                    .await?;
                self.embed_devlogs_from_db(embedding_service, &pool).await?;
            }
        }

        tracing::info!("Reform embedding job completed successfully");
        Ok(())
    }

    fn name(&self) -> &str {
        "ReformJob"
    }
}
