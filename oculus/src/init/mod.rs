pub mod embed;

use crate::core::progress::ProgressReporter;
use crate::core::{with_retry, Job, JobError};
use async_trait::async_trait;
use common::{
    database::connection::{create_pool, run_migrations},
    services::{external::ExternalApiService, EmbeddingService},
    utils::config::Config,
    DbPool,
};
use std::collections::HashSet;
use std::sync::Arc;

use self::embed::InitEmbedder;

const DEV_MODE_MAX_PAGES: i32 = 5;

pub struct InitJob {
    config: Config,
    embedding_service: Arc<EmbeddingService>,
}

impl InitJob {
    pub fn new(config: Config, embedding_service: Arc<EmbeddingService>) -> Self {
        Self {
            config,
            embedding_service,
        }
    }

    async fn fetch_all_projects(
        &self,
        external_api: &ExternalApiService,
    ) -> Result<Vec<common::utils::modal::RawProject>, JobError> {
        let mut all_projects = Vec::new();
        let mut page = 1;
        let max_pages = if std::env::var("DEV_MODE").unwrap_or_default().to_lowercase() == "true" {
            DEV_MODE_MAX_PAGES
        } else {
            i32::MAX
        };

        loop {
            let response = with_retry(&format!("fetch_projects_page_{}", page), || {
                external_api.fetch_projects(Some(page))
            })
            .await
            .map_err(|e| JobError::ExternalApi(e.to_string()))?;

            if response.projects.is_empty() || page >= max_pages {
                break;
            }

            all_projects.extend(response.projects);

            if let Some(pagination) = response.pagination {
                if let Some(total_pages) = pagination.pages {
                    let progress = (page as f64 / total_pages as f64 * 100.0) as u32;
                    print!(
                        "\rFetching projects: {}% ({}/{})",
                        progress, page, total_pages
                    );
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                    if page >= total_pages {
                        break;
                    }
                }
            }

            page += 1;
        }
        println!();
        Ok(all_projects)
    }

    async fn fetch_all_comments(
        &self,
        external_api: &ExternalApiService,
    ) -> Result<Vec<common::utils::modal::RawComment>, JobError> {
        let mut all_comments = Vec::new();
        let mut page = 1;
        let max_pages = if std::env::var("DEV_MODE").unwrap_or_default().to_lowercase() == "true" {
            DEV_MODE_MAX_PAGES
        } else {
            i32::MAX
        };

        loop {
            let response = with_retry(&format!("fetch_comments_page_{}", page), || {
                external_api.fetch_comments(Some(page))
            })
            .await
            .map_err(|e| JobError::ExternalApi(e.to_string()))?;

            if response.comments.is_empty() || page >= max_pages {
                break;
            }

            all_comments.extend(response.comments);

            if let Some(pagination) = response.pagination {
                if let Some(total_pages) = pagination.pages {
                    let progress = (page as f64 / total_pages as f64 * 100.0) as u32;
                    print!(
                        "\rFetching comments: {}% ({}/{})",
                        progress, page, total_pages
                    );
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                    if page >= total_pages {
                        break;
                    }
                }
            }

            page += 1;
        }
        println!();
        Ok(all_comments)
    }

    async fn fetch_all_devlogs(
        &self,
        external_api: &ExternalApiService,
    ) -> Result<Vec<common::utils::modal::RawDevlog>, JobError> {
        let mut all_devlogs = Vec::new();
        let mut page = 1;
        let max_pages = if std::env::var("DEV_MODE").unwrap_or_default().to_lowercase() == "true" {
            DEV_MODE_MAX_PAGES
        } else {
            i32::MAX
        };

        loop {
            let response = with_retry(&format!("fetch_devlogs_page_{}", page), || {
                external_api.fetch_devlogs(Some(page))
            })
            .await
            .map_err(|e| JobError::ExternalApi(e.to_string()))?;

            if response.devlogs.is_empty() || page >= max_pages {
                break;
            }

            all_devlogs.extend(response.devlogs);

            if let Some(pagination) = response.pagination {
                if let Some(total_pages) = pagination.pages {
                    let progress = (page as f64 / total_pages as f64 * 100.0) as u32;
                    print!(
                        "\rFetching devlogs: {}% ({}/{})",
                        progress, page, total_pages
                    );
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                    if page >= total_pages {
                        break;
                    }
                }
            }

            page += 1;
        }
        println!();
        Ok(all_devlogs)
    }

    async fn store_raw_data(
        &self,
        projects: Vec<common::utils::modal::RawProject>,
        devlogs: Vec<common::utils::modal::RawDevlog>,
        comments: Vec<common::utils::modal::RawComment>,
        pool: &common::database::connection::DbPool,
    ) -> Result<(), JobError> {
        let mut client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;
        let tx = client
            .transaction()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let total_projects = projects.len();
        let projects_progress = ProgressReporter::new_with_job("init", "Storing projects");
        for (i, project) in projects.iter().enumerate() {
            projects_progress.report(i + 1, total_projects);
            tx.execute(
                r#"INSERT INTO projects (id, title, description, readme_link, slack_id, created_at, updated_at)
                   VALUES ($1, $2, $3, $4, $5, $6, $7)
                   ON CONFLICT (id) DO UPDATE SET 
                       title = EXCLUDED.title,
                       description = COALESCE(EXCLUDED.description, projects.description),
                       readme_link = COALESCE(EXCLUDED.readme_link, projects.readme_link),
                       updated_at = EXCLUDED.updated_at"#,
                &[
                    &project.id,
                    &project.title,
                    &project.description,
                    &project.readme_link,
                    &project.slack_id,
                    &crate::core::parse_datetime(&project.created_at)?,
                    &crate::core::parse_datetime(&project.updated_at)?,
                ]
            ).await.map_err(|e| JobError::Database(e.to_string()))?;
        }
        projects_progress.finish();

        let project_ids: HashSet<i64> = projects.iter().map(|p| p.id).collect();

        let total_devlogs = devlogs.len();
        let devlogs_progress = ProgressReporter::new_with_job("init", "Storing devlogs");
        let mut valid_devlog_ids: HashSet<i64> = HashSet::with_capacity(total_devlogs);
        for (i, devlog) in devlogs.iter().enumerate() {
            if !project_ids.contains(&devlog.project_id) {
                continue;
            }
            devlogs_progress.report(i + 1, total_devlogs);
            tx.execute(
                r#"INSERT INTO logs (id, text, project_id, slack_id, created_at, updated_at)
                   VALUES ($1, $2, $3, $4, $5, $6)
                   ON CONFLICT (id) DO UPDATE SET text = EXCLUDED.text, updated_at = EXCLUDED.updated_at"#,
                &[
                    &devlog.id,
                    &devlog.text,
                    &devlog.project_id,
                    &devlog.slack_id,
                    &crate::core::parse_datetime(&devlog.created_at)?,
                    &crate::core::parse_datetime(&devlog.updated_at)?,
                ]
            ).await.map_err(|e| JobError::Database(e.to_string()))?;
            valid_devlog_ids.insert(devlog.id);
        }
        devlogs_progress.finish();

        let total_comments = comments.len();
        let comments_progress = ProgressReporter::new_with_job("init", "Storing comments");
        for (i, comment) in comments.iter().enumerate() {
            if !valid_devlog_ids.contains(&comment.devlog_id) {
                continue;
            }
            comments_progress.report(i + 1, total_comments);
            tx.execute(
                r#"INSERT INTO comments (text, devlog_id, slack_id, created_at)
                   VALUES ($1, $2, $3, $4)
                   ON CONFLICT (devlog_id, slack_id) DO UPDATE SET text = EXCLUDED.text"#,
                &[
                    &comment.text,
                    &comment.devlog_id,
                    &comment.slack_id,
                    &crate::core::parse_datetime(&comment.created_at)?,
                ],
            )
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;
        }
        comments_progress.finish();

        tx.commit()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;
        Ok(())
    }

    async fn wipe_database(&self) -> Result<(), JobError> {
        Ok(())
    }

    async fn sync_user_data_from_leaderboard(
        &self,
        external_api: &ExternalApiService,
        pool: &common::database::connection::DbPool,
    ) -> Result<(), JobError> {
        tracing::info!("Syncing user data from leaderboard");

        let leaderboard_response = external_api
            .fetch_leaderboard()
            .await
            .map_err(|e| JobError::ExternalApi(format!("Failed to fetch leaderboard: {}", e)))?;

        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let total = leaderboard_response.users.len();
        let progress = ProgressReporter::new_with_job("init", "Syncing user leaderboard data");

        for (i, user) in leaderboard_response.users.iter().enumerate() {
            progress.report(i + 1, total);

            client
                .execute(
                    r#"
                INSERT INTO users (slack_id, username, current_shells, last_synced, pfp_url) 
                VALUES ($1, $2, $3, NOW(), 'notfound')
                ON CONFLICT (slack_id) DO UPDATE SET 
                    username = COALESCE(EXCLUDED.username, users.username),
                    current_shells = EXCLUDED.current_shells,
                    last_synced = EXCLUDED.last_synced
                "#,
                    &[&user.slack_id, &user.username, &user.shells],
                )
                .await
                .map_err(|e| JobError::Database(e.to_string()))?;

            if let Some(payouts) = &user.payouts {
                self.process_user_payouts(&user.slack_id, user.shells, payouts, &client)
                    .await?;
            }
        }

        progress.finish();
        Ok(())
    }

    async fn process_user_payouts(
        &self,
        slack_id: &str,
        final_shells: i32,
        payouts: &[common::utils::modal::RawPayout],
        client: &tokio_postgres::Client,
    ) -> Result<(), JobError> {
        let mut sorted_payouts = payouts.to_vec();
        sorted_payouts.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        let mut running_shells = final_shells;
        let mut shell_history_entries = Vec::new();

        for payout in sorted_payouts.iter().rev() {
            let shell_diff = payout.amount.parse::<f64>().map_err(|e| {
                JobError::Database(format!("Invalid payout amount '{}': {}", payout.amount, e))
            })? as i32;

            let shells_then = running_shells - shell_diff;

            shell_history_entries.push((
                crate::core::parse_datetime(&payout.created_at)?,
                shells_then,
                shell_diff,
                running_shells,
            ));

            running_shells = shells_then;
        }

        shell_history_entries.reverse();

        for (recorded_at, shells_then, shell_diff, shells) in shell_history_entries {
            client
                .execute(
                    r#"
                INSERT INTO shell_history (slack_id, shells_then, shell_diff, shells, recorded_at)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (slack_id, recorded_at) DO NOTHING
                "#,
                    &[
                        &slack_id,
                        &Some(shells_then),
                        &shell_diff,
                        &shells,
                        &recorded_at,
                    ],
                )
                .await
                .map_err(|e| JobError::Database(e.to_string()))?;
        }

        Ok(())
    }

    async fn ensure_users_exist(
        &self,
        projects: &[common::utils::modal::RawProject],
        comments: &[common::utils::modal::RawComment],
        devlogs: &[common::utils::modal::RawDevlog],
        pool: &common::database::connection::DbPool,
    ) -> Result<(), JobError> {
        let mut slack_ids = HashSet::new();

        for project in projects {
            slack_ids.insert(&project.slack_id);
        }

        for comment in comments {
            slack_ids.insert(&comment.slack_id);
        }

        for devlog in devlogs {
            slack_ids.insert(&devlog.slack_id);
        }

        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let total = slack_ids.len();
        let users_progress = ProgressReporter::new_with_job("init", "Creating users");

        for (i, slack_id) in slack_ids.iter().enumerate() {
            users_progress.report(i + 1, total);

            client
                .execute(
                    r#"
                INSERT INTO users (slack_id, pfp_url) 
                VALUES ($1, 'notfound')
                ON CONFLICT (slack_id) DO NOTHING
                "#,
                    &[slack_id],
                )
                .await
                .map_err(|e| JobError::Database(e.to_string()))?;
        }
        users_progress.finish();

        Ok(())
    }
}

#[async_trait]
impl Job for InitJob {
    async fn execute(&self, _: &DbPool) -> Result<(), JobError> {
        let pool = Arc::new(
            create_pool(&self.config)
                .await
                .map_err(|e| JobError::Database(e.to_string()))?,
        );

        run_migrations(&pool)
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let should_wipe = std::env::var("WIPE")
            .unwrap_or_else(|_| "false".to_string())
            .to_lowercase()
            == "true";
        if should_wipe {
            tracing::warn!("WIPING DATABASE - This will delete ALL data!");
            self.wipe_database().await?;
            tracing::warn!("Database wipe completed");
        }

        let external_api = Arc::new(
            ExternalApiService::new(self.config.journey_session_cookie.clone())
                .map_err(|e| JobError::ExternalApi(e.to_string()))?,
        );

        tracing::info!("Fetching all projects from API");
        let projects = self.fetch_all_projects(&external_api).await?;
        tracing::info!("Fetched {} projects", projects.len());

        tracing::info!("Fetching all comments from API");
        let comments = self.fetch_all_comments(&external_api).await?;
        tracing::info!("Fetched {} comments", comments.len());

        tracing::info!("Fetching all devlogs from API");
        let devlogs = self.fetch_all_devlogs(&external_api).await?;
        tracing::info!("Fetched {} devlogs", devlogs.len());

        tracing::info!("Creating user records from extracted slack_ids");
        self.ensure_users_exist(&projects, &comments, &devlogs, &pool)
            .await?;

        tracing::info!("Syncing user shell data from leaderboard");
        self.sync_user_data_from_leaderboard(&external_api, &pool)
            .await?;

        tracing::info!("Storing raw data in database");
        self.store_raw_data(projects.clone(), devlogs.clone(), comments.clone(), &pool)
            .await?;

        tracing::info!("Embedding all data");
        InitEmbedder::embed_projects(&projects, Arc::clone(&self.embedding_service), &pool).await?;
        InitEmbedder::embed_devlogs(&devlogs, Arc::clone(&self.embedding_service), &pool).await?;
        InitEmbedder::embed_comments(&comments, Arc::clone(&self.embedding_service), &pool).await?;

        tracing::info!("Initial synchronization completed successfully");
        Ok(())
    }

    fn name(&self) -> &str {
        "InitJob"
    }
}