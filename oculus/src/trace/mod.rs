use crate::core::{progress::get_job_progress, Job, JobError};
use async_trait::async_trait;
use common::{database::DbPool, services::external::ExternalApiService, utils::config::Config};
use futures::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;

mod slack;
mod trust;
mod update;

use slack::SlackManager;
use trust::TrustManager;
use update::UserUpdater;

pub struct TraceJob {
    config: Config,
}

impl TraceJob {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Job for TraceJob {
    async fn execute(&self, pool: &DbPool) -> Result<(), JobError> {
        let pool = Arc::new(pool.clone());

        let external_api = Arc::new(
            ExternalApiService::new(self.config.journey_session_cookie.clone())
                .map_err(|e| JobError::ExternalApi(e.to_string()))?,
        );

        let slack_manager = Arc::new(SlackManager::new(self.config.clone()));

        let users_needing_info = UserUpdater::find_users_needing_info(&pool).await?;

        if users_needing_info.is_empty() {
            return Err(JobError::Other("no_work".to_string()));
        }

        let total_users = users_needing_info.len();

        let progress = get_job_progress("trace");
        progress.update_progress(0, total_users, "Processing users concurrently");

        let mut futures = FuturesUnordered::new();

        for (idx, slack_id) in users_needing_info.into_iter().enumerate() {
            let pool = Arc::clone(&pool);
            let external_api = Arc::clone(&external_api);
            let slack_manager = Arc::clone(&slack_manager);

            let future = async move {
                let slack_result = match slack_manager.fetch_user_info_from_slack(&slack_id).await {
                    Ok(Some((username, profile))) => {
                        UserUpdater::update_user_with_slack_info(
                            &pool, &slack_id, &username, &profile,
                        )
                        .await
                        .ok();
                        Some(())
                    }
                    Ok(None) => None,
                    Err(JobError::Other(ref err)) if err == "rate_limited" => {
                        None
                    }
                    Err(_) => None,
                };

                let trust_result =
                    match TrustManager::fetch_trust_info(&external_api, &slack_id).await {
                        Ok(Some((trust_level, trust_value))) => {
                            UserUpdater::update_user_with_trust_info(
                                &pool,
                                &slack_id,
                                &trust_level,
                                trust_value,
                            )
                            .await
                            .ok();
                            Some(())
                        }
                        Ok(None) => None,
                        Err(_) => None,
                    };

                Result::<(usize, bool, bool), JobError>::Ok((
                    idx,
                    slack_result.is_some(),
                    trust_result.is_some(),
                ))
            };

            futures.push(future);
        }

        let mut completed = 0;
        let mut slack_updated = 0;
        let mut trust_updated = 0;

        while let Some(result) = futures.next().await {
            match result {
                Ok((_, slack_success, trust_success)) => {
                    completed += 1;
                    if slack_success {
                        slack_updated += 1;
                    }
                    if trust_success {
                        trust_updated += 1;
                    }
                    progress.update_progress(
                        completed,
                        total_users,
                        &format!(
                            "Processing users... (Slack: {}, Trust: {})",
                            slack_updated, trust_updated
                        ),
                    );
                }
                Err(_) => {
                    completed += 1;
                    progress.update_progress(
                        completed,
                        total_users,
                        &format!(
                            "Processing users... (Slack: {}, Trust: {})",
                            slack_updated, trust_updated
                        ),
                    );
                }
            }
        }

        progress.done(format!(
            "Processed {} users (Slack: {}, Trust: {} updated)",
            total_users, slack_updated, trust_updated
        ));

        Ok(())
    }

    fn name(&self) -> &str {
        "TraceJob"
    }
}
