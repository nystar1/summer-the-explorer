use crate::core::JobError;
use common::{
    database::DbPool,
    services::EmbeddingService,
    utils::modal::{RawComment, RawDevlog, RawProject},
};

pub struct DataStore;

impl DataStore {
    pub async fn store_project_with_embedding(
        project: &RawProject,
        embedding_service: &EmbeddingService,
        pool: &DbPool,
    ) -> Result<(), JobError> {
        let text = format!(
            "{} {}",
            project.title,
            project.description.as_deref().unwrap_or_default()
        )
        .trim()
        .to_string();

        let embedding_vec = embedding_service
            .embed_text(&text)
            .await
            .map_err(|e| JobError::Embedding(e.to_string()))?;

        let embedding = pgvector::Vector::from(embedding_vec);

        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let created_at = crate::core::parse_datetime(&project.created_at)?;
        let updated_at = crate::core::parse_datetime(&project.updated_at)?;

        client
            .execute(
                r#"
            INSERT INTO projects (
                id, title, description, readme_link, slack_id, created_at, updated_at, 
                title_description_embedding
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (id) DO NOTHING
            "#,
                &[
                    &project.id,
                    &project.title,
                    &project.description,
                    &project.readme_link,
                    &project.slack_id,
                    &created_at,
                    &updated_at,
                    &embedding,
                ],
            )
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        Ok(())
    }

    pub async fn store_comment_with_embedding(
        comment: &RawComment,
        embedding_service: &EmbeddingService,
        pool: &DbPool,
    ) -> Result<(), JobError> {
        let embedding_vec = embedding_service
            .embed_text(&comment.text)
            .await
            .map_err(|e| JobError::Embedding(e.to_string()))?;

        let embedding = pgvector::Vector::from(embedding_vec);

        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let created_at = crate::core::parse_datetime(&comment.created_at)?;

        let devlog_exists = client
            .query("SELECT 1 FROM logs WHERE id = $1", &[&comment.devlog_id])
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        if !devlog_exists.is_empty() {
            client
                .execute(
                    r#"
                INSERT INTO comments (
                    text, devlog_id, slack_id, created_at, text_embedding
                ) VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (devlog_id, slack_id) DO NOTHING
                "#,
                    &[
                        &comment.text,
                        &comment.devlog_id,
                        &comment.slack_id,
                        &created_at,
                        &embedding,
                    ],
                )
                .await
                .map_err(|e| JobError::Database(e.to_string()))?;
        } else {
            tracing::debug!(
                "Skipping comment for devlog {} - devlog no longer exists",
                comment.devlog_id
            );
        }

        Ok(())
    }

    pub async fn store_devlog_with_embedding(
        devlog: &RawDevlog,
        embedding_service: &EmbeddingService,
        pool: &DbPool,
    ) -> Result<(), JobError> {
        let embedding_vec = embedding_service
            .embed_text(&devlog.text)
            .await
            .map_err(|e| JobError::Embedding(e.to_string()))?;

        let embedding = pgvector::Vector::from(embedding_vec);

        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let created_at = crate::core::parse_datetime(&devlog.created_at)?;
        let updated_at = crate::core::parse_datetime(&devlog.updated_at)?;

        let project_exists = client
            .query(
                "SELECT 1 FROM projects WHERE id = $1",
                &[&devlog.project_id],
            )
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        if !project_exists.is_empty() {
            client
                .execute(
                    r#"
                INSERT INTO logs (
                    id, text, project_id, slack_id, created_at, updated_at, text_embedding
                ) VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (id) DO NOTHING
                "#,
                    &[
                        &devlog.id,
                        &devlog.text,
                        &devlog.project_id,
                        &devlog.slack_id,
                        &created_at,
                        &updated_at,
                        &embedding,
                    ],
                )
                .await
                .map_err(|e| JobError::Database(e.to_string()))?;
        } else {
            tracing::debug!(
                "Skipping devlog {} for project {} - project no longer exists",
                devlog.id,
                devlog.project_id
            );
        }

        Ok(())
    }
}
