use crate::core::{get_base_concurrency, JobError};
use common::{
    database::connection,
    services::EmbeddingService,
    utils::modal::{RawProject, RawComment, RawDevlog}
};
use futures::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;
use tokio::sync::Semaphore;
use indicatif::{ProgressBar, ProgressStyle};

const DEFAULT_EMBED_BATCH_SIZE: usize = 32;
const MAX_DB_EMBED_CONCURRENCY: usize = 8;

pub struct InitEmbedder;

impl InitEmbedder {
    fn get_embed_batch_size() -> usize {
        std::env::var("EMBED_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_EMBED_BATCH_SIZE) 
    }

    fn get_db_concurrency() -> usize {
        std::env::var("DB_EMBED_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or_else(|| get_base_concurrency().min(MAX_DB_EMBED_CONCURRENCY))
    }

    pub async fn embed_projects(
        projects: &[RawProject],
        embedding_service: Arc<EmbeddingService>,
        pool: &connection::DbPool,
    ) -> Result<(), JobError> {
        if projects.is_empty() {
            return Ok(());
        }

        let embed_batch_size = Self::get_embed_batch_size();
        let db_concurrency = Self::get_db_concurrency();
        
        let start_time = std::time::Instant::now();
        let progress = ProgressBar::new(projects.len() as u64);
        progress.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} projects ({percent}%) {msg}")
                .unwrap()
                .progress_chars("#>-")
        );
        progress.set_message("Processing embeddings...");
        
        let pool = Arc::new(pool.clone());
        
        
        let db_semaphore = Arc::new(Semaphore::new(db_concurrency));
        
        
        let mut processed = 0;
        
        for chunk in projects.chunks(embed_batch_size) {
            
            let texts: Vec<String> = chunk.iter()
                .map(|p| format!("{} {}", p.title, p.description.as_deref().unwrap_or("")))
                .collect();
            
            let embeddings = embedding_service.embed_batch(texts).await
                .map_err(|e| JobError::Embedding(e.to_string()))?;
            
            
            let mut futures = FuturesUnordered::new();
            
            for (project, embedding) in chunk.iter().zip(embeddings.iter()) {
                let db_semaphore = db_semaphore.clone();
                let pool = pool.clone();
                let project_id = project.id;
                let embedding = serde_json::to_string(embedding)
                    .map_err(|e| JobError::Embedding(format!("Failed to serialize embedding: {}", e)))?;
                
                let future = async move {
                    let _permit = db_semaphore.acquire().await.map_err(|e| {
                        JobError::Database(format!("Semaphore error: {}", e))
                    })?;
                    
                    let client = pool.get().await.map_err(|e| JobError::Database(e.to_string()))?;
                    client.execute(
                        "UPDATE projects SET embedding = $2 WHERE id = $1",
                        &[&project_id, &embedding],
                    ).await.map_err(|e| JobError::Database(e.to_string()))?;
                    
                    Result::<(), JobError>::Ok(())
                };
                
                futures.push(future);
            }
            
            
            while let Some(result) = futures.next().await {
                result?; 
                processed += 1;
                progress.set_position(processed as u64);
                progress.set_message(format!("Processed {} projects", processed));
            }
        }
        
        let elapsed = start_time.elapsed();
        progress.finish_with_message(format!("✅ All {} project embeddings completed in {:.2}s", projects.len(), elapsed.as_secs_f64()));
        Ok(())
    }

    pub async fn embed_comments(
        comments: &[RawComment],
        embedding_service: Arc<EmbeddingService>,
        pool: &connection::DbPool,
    ) -> Result<(), JobError> {
        if comments.is_empty() {
            return Ok(());
        }

        let embed_batch_size = Self::get_embed_batch_size();
        let db_concurrency = Self::get_db_concurrency();
        
        let start_time = std::time::Instant::now();
        let progress = ProgressBar::new(comments.len() as u64);
        progress.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} comments ({percent}%) {msg}")
                .unwrap()
                .progress_chars("#>-")
        );
        progress.set_message("Processing embeddings...");
        
        let pool = Arc::new(pool.clone());
        
        
        let db_semaphore = Arc::new(Semaphore::new(db_concurrency));
        
        
        let mut processed = 0;
        
        for chunk in comments.chunks(embed_batch_size) {
            
            let texts: Vec<String> = chunk.iter()
                .map(|c| c.text.clone())
                .collect();
            
            let embeddings = embedding_service.embed_batch(texts).await
                .map_err(|e| JobError::Embedding(e.to_string()))?;
            
            
            let mut futures = FuturesUnordered::new();
            
            for (comment, embedding) in chunk.iter().zip(embeddings.iter()) {
                let db_semaphore = db_semaphore.clone();
                let pool = pool.clone();
                let devlog_id = comment.devlog_id;
                let slack_id = comment.slack_id.clone();
                let embedding = serde_json::to_string(embedding)
                    .map_err(|e| JobError::Embedding(format!("Failed to serialize embedding: {}", e)))?;
                
                let future = async move {
                    let _permit = db_semaphore.acquire().await.map_err(|e| {
                        JobError::Database(format!("Semaphore error: {}", e))
                    })?;
                    
                    let client = pool.get().await.map_err(|e| JobError::Database(e.to_string()))?;
                    client.execute(
                        "UPDATE comments SET embedding = $3 WHERE devlog_id = $1 AND slack_id = $2",
                        &[&devlog_id, &slack_id, &embedding],
                    ).await.map_err(|e| JobError::Database(e.to_string()))?;
                    
                    Result::<(), JobError>::Ok(())
                };
                
                futures.push(future);
            }
            
            
            while let Some(result) = futures.next().await {
                result?; 
                processed += 1;
                progress.set_position(processed as u64);
                progress.set_message(format!("Processed {} comments", processed));
            }
        }
        
        let elapsed = start_time.elapsed();
        progress.finish_with_message(format!("✅ All {} comment embeddings completed in {:.2}s", comments.len(), elapsed.as_secs_f64()));
        Ok(())
    }

    pub async fn embed_devlogs(
        devlogs: &[RawDevlog],
        embedding_service: Arc<EmbeddingService>,
        pool: &connection::DbPool,
    ) -> Result<(), JobError> {
        if devlogs.is_empty() {
            return Ok(());
        }

        let embed_batch_size = Self::get_embed_batch_size();
        let db_concurrency = Self::get_db_concurrency();
        
        let start_time = std::time::Instant::now();
        let progress = ProgressBar::new(devlogs.len() as u64);
        progress.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} devlogs ({percent}%) {msg}")
                .unwrap()
                .progress_chars("#>-")
        );
        progress.set_message("Processing embeddings...");
        
        let pool = Arc::new(pool.clone());
        
        
        let db_semaphore = Arc::new(Semaphore::new(db_concurrency));
        
        
        let mut processed = 0;
        
        for chunk in devlogs.chunks(embed_batch_size) {
            
            let texts: Vec<String> = chunk.iter()
                .map(|d| d.text.clone())
                .collect();
            
            let embeddings = embedding_service.embed_batch(texts).await
                .map_err(|e| JobError::Embedding(e.to_string()))?;
            
            
            let mut futures = FuturesUnordered::new();
            
            for (devlog, embedding) in chunk.iter().zip(embeddings.iter()) {
                let db_semaphore = db_semaphore.clone();
                let pool = pool.clone();
                let devlog_id = devlog.id;
                let embedding = serde_json::to_string(embedding)
                    .map_err(|e| JobError::Embedding(format!("Failed to serialize embedding: {}", e)))?;
                
                let future = async move {
                    let _permit = db_semaphore.acquire().await.map_err(|e| {
                        JobError::Database(format!("Semaphore error: {}", e))
                    })?;
                    
                    let client = pool.get().await.map_err(|e| JobError::Database(e.to_string()))?;
                    client.execute(
                        "UPDATE devlogs SET embedding = $2 WHERE id = $1",
                        &[&devlog_id, &embedding],
                    ).await.map_err(|e| JobError::Database(e.to_string()))?;
                    
                    Result::<(), JobError>::Ok(())
                };
                
                futures.push(future);
            }
            
            
            while let Some(result) = futures.next().await {
                result?; 
                processed += 1;
                progress.set_position(processed as u64);
                progress.set_message(format!("Processed {} devlogs", processed));
            }
        }
        
        let elapsed = start_time.elapsed();
        progress.finish_with_message(format!("✅ All {} devlog embeddings completed in {:.2}s", devlogs.len(), elapsed.as_secs_f64()));
        Ok(())
    }
}