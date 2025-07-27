use crate::core::{Job, JobError};
use async_trait::async_trait;
use common::{
    database::manager::ConnectionManager, services::external::ExternalApiService,
    utils::config::Config,
};
use std::collections::HashMap;
use tokio_postgres::Client;

pub struct ZenithJob {
    config: Config,
}

impl ZenithJob {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    async fn sync_leaderboard_data(&self, pool: &common::database::DbPool) -> Result<(), JobError> {
        tracing::info!("Starting leaderboard sync");

        let external_api = ExternalApiService::new(self.config.journey_session_cookie.clone())
            .map_err(|e| {
                JobError::ExternalApi(format!("Failed to create external API service: {}", e))
            })?;

        let leaderboard_response = external_api
            .fetch_leaderboard()
            .await
            .map_err(|e| JobError::ExternalApi(format!("Failed to fetch leaderboard: {}", e)))?;

        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;
        let current_users = self.get_current_users(&client).await?;
        let mut updated_count = 0;
        let mut new_count = 0;

        tracing::info!(
            "Processing {} leaderboard users, {} users in database",
            leaderboard_response.users.len(),
            current_users.len()
        );

        let total_users = leaderboard_response.users.len();
        for (i, user) in leaderboard_response.users.iter().enumerate() {
            let user_exists = current_users.contains_key(&user.slack_id);

            if total_users <= 10 || (i + 1) % (total_users / 10).max(1) == 0 {
                tracing::info!(
                    "Processing user {}/{}: {} shells",
                    i + 1,
                    total_users,
                    user.shells
                );
            }

            if user_exists {
                let current_shells = current_users.get(&user.slack_id).unwrap();
                let needs_update = match current_shells {
                    None => true,                        
                    Some(shells) => *shells != user.shells,
                };

                if needs_update {
                    self.update_user_shells(&client, &user.slack_id, user.shells)
                        .await?;
                    updated_count += 1;
                }
            } else {
                self.create_new_user(&client, user).await?;
                new_count += 1;
            }

            if let Some(payouts) = &user.payouts {
                if !payouts.is_empty() {
                    self.process_user_payouts(&client, &user.slack_id, payouts, user.shells)
                        .await?;
                }
            }
        }

        tracing::info!(
            "Leaderboard sync complete: {} updated, {} new users",
            updated_count,
            new_count
        );
        Ok(())
    }

    async fn get_current_users(
        &self,
        client: &Client,
    ) -> Result<HashMap<String, Option<i32>>, JobError> {
        let rows = client
            .query("SELECT slack_id, current_shells FROM users", &[])
            .await
            .map_err(|e| JobError::Database(format!("Failed to get current users: {}", e)))?;

        let mut users = HashMap::new();
        for row in rows {
            let slack_id: String = row.get(0);
            let shells: Option<i32> = row.get(1);
            users.insert(slack_id, shells);
        }

        Ok(users)
    }

    async fn update_user_shells(
        &self,
        client: &Client,
        slack_id: &str,
        new_shells: i32,
    ) -> Result<(), JobError> {
        client
            .execute(
                "UPDATE users SET current_shells = $1 WHERE slack_id = $2",
                &[&new_shells, &slack_id],
            )
            .await
            .map_err(|e| JobError::Database(format!("Failed to update user shells: {}", e)))?;

        Ok(())
    }

    async fn create_new_user(
        &self,
        client: &Client,
        user: &common::utils::modal::RawLeaderboardEntry,
    ) -> Result<(), JobError> {
        client
            .execute(
                "INSERT INTO users (slack_id, username, current_shells, last_synced, pfp_url) VALUES ($1, $2, $3, NOW(), 'notfound') ON CONFLICT (slack_id) DO NOTHING",
                &[&user.slack_id, &user.username, &user.shells]
            )
            .await
            .map_err(|e| JobError::Database(format!("Failed to create new user: {}", e)))?;

        Ok(())
    }

    async fn process_user_payouts(
        &self,
        client: &Client,
        slack_id: &str,
        payouts: &[common::utils::modal::RawPayout],
        final_shells: i32,
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
                chrono::DateTime::parse_from_rfc3339(&payout.created_at)
                    .map_err(|e| {
                        JobError::Database(format!(
                            "Invalid payout created_at '{}': {}",
                            payout.created_at, e
                        ))
                    })?
                    .with_timezone(&chrono::Utc),
                shells_then,
                shell_diff,
                running_shells,
            ));

            running_shells = shells_then;
        }

        shell_history_entries.reverse();

        for (recorded_at, shells_then, shell_diff, shells) in shell_history_entries {
            client.execute(
                "INSERT INTO shell_history (slack_id, shells_then, shell_diff, shells, recorded_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (slack_id, recorded_at) DO NOTHING",
                &[&slack_id, &Some(shells_then), &shell_diff, &shells, &recorded_at]
            ).await
            .map_err(|e| JobError::Database(format!("Failed to insert shell history: {}", e)))?;
        }

        Ok(())
    }
}

#[async_trait]
impl Job for ZenithJob {
    fn name(&self) -> &str {
        "ZenithJob"
    }

    async fn execute(&self, _: &common::database::DbPool) -> Result<(), JobError> {
        let pool = ConnectionManager::get_dedicated_pool(&self.config)
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        self.sync_leaderboard_data(&pool).await?;

        tracing::info!("Zenith job completed, releasing dedicated connection");
        Ok(())
    }
}
