use crate::core::JobError;
use common::{database::connection, services::external::ExternalApiService};

pub struct DataSyncer;

impl DataSyncer {
    pub async fn get_last_sync_metadata(
        pool: &connection::DbPool,
        key: &str,
    ) -> Result<Option<(chrono::DateTime<chrono::Utc>, i32)>, JobError> {
        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let rows = client
            .query(
                "SELECT last_sync, last_page FROM sync_metadata WHERE key = $1",
                &[&key],
            )
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        if let Some(row) = rows.first() {
            let last_sync: Option<chrono::DateTime<chrono::Utc>> = row.get(0);
            let last_page: Option<i32> = row.get(1);
            Ok(last_sync.zip(last_page))
        } else {
            Ok(None)
        }
    }

    pub async fn update_sync_metadata(
        pool: &connection::DbPool,
        key: &str,
        page: i32,
    ) -> Result<(), JobError> {
        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        client.execute(
            "INSERT INTO sync_metadata (key, last_sync, last_page, status) VALUES ($1, NOW(), $2, 'completed') ON CONFLICT (key) DO UPDATE SET last_sync = NOW(), last_page = $2, status = 'completed'",
            &[&key, &page]
        ).await
        .map_err(|e| JobError::Database(e.to_string()))?;

        Ok(())
    }

    pub async fn calculate_start_page(pool: &connection::DbPool) -> Result<i32, JobError> {
        if let Some((_, last_page)) = Self::get_last_sync_metadata(pool, "projects").await? {
            Ok(last_page + 1)
        } else {
            Ok(1)
        }
    }

    pub async fn sync_user_shell_data(
        external_api: &ExternalApiService,
        pool: &connection::DbPool,
    ) -> Result<(), JobError> {
        tracing::info!("Syncing user shell data from leaderboard");

        let leaderboard_response = external_api
            .fetch_leaderboard()
            .await
            .map_err(|e| JobError::ExternalApi(format!("Failed to fetch leaderboard: {}", e)))?;

        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let mut updated_count = 0;

        for user in leaderboard_response.users.iter() {
            let current_shells_row = client
                .query(
                    "SELECT current_shells FROM users WHERE slack_id = $1",
                    &[&user.slack_id],
                )
                .await
                .map_err(|e| JobError::Database(e.to_string()))?;

            let current_shells: Option<i32> = current_shells_row
                .first()
                .and_then(|row| row.get::<_, Option<i32>>(0));

            let rows_affected = client
                .execute(
                    r#"
                INSERT INTO users (slack_id, username, current_shells, last_synced, pfp_url)
                VALUES ($1, $2, $3, NOW(), 'notfound')
                ON CONFLICT (slack_id) DO UPDATE SET 
                    username = COALESCE(EXCLUDED.username, users.username),
                    current_shells = EXCLUDED.current_shells,
                    last_synced = EXCLUDED.last_synced
                WHERE users.current_shells IS DISTINCT FROM EXCLUDED.current_shells
                "#,
                    &[&user.slack_id, &user.username, &user.shells],
                )
                .await
                .map_err(|e| JobError::Database(e.to_string()))?;

            if rows_affected > 0 {
                updated_count += 1;

                if let Some(payouts) = &user.payouts {
                    Self::process_new_payouts(
                        &user.slack_id,
                        current_shells,
                        payouts,
                        &client,
                    )
                    .await?;
                }
            }
        }

        tracing::info!("Updated shell data for {} users", updated_count);
        Ok(())
    }

    async fn process_new_payouts(
        slack_id: &str,
        previous_shells: Option<i32>,
        payouts: &[common::utils::modal::RawPayout],
        client: &tokio_postgres::Client,
    ) -> Result<(), JobError> {
        let last_history_row = client.query(
            "SELECT recorded_at FROM shell_history WHERE slack_id = $1 ORDER BY recorded_at DESC LIMIT 1",
            &[&slack_id]
        ).await
        .map_err(|e| JobError::Database(e.to_string()))?;

        let last_recorded: Option<chrono::DateTime<chrono::Utc>> = last_history_row
            .first()
            .and_then(|row| row.get::<_, Option<chrono::DateTime<chrono::Utc>>>(0));

        let mut new_payouts = Vec::with_capacity(payouts.len());
        for payout in payouts {
            let payout_time = chrono::DateTime::parse_from_rfc3339(&payout.created_at)
                .map_err(|e| {
                    JobError::Database(format!(
                        "Invalid payout created_at '{}': {}",
                        payout.created_at, e
                    ))
                })?
                .with_timezone(&chrono::Utc);

            if let Some(last_recorded) = last_recorded {
                if payout_time > last_recorded {
                    new_payouts.push(payout);
                }
            } else {
                new_payouts.push(payout);
            }
        }

        if new_payouts.is_empty() {
            return Ok(());
        }

        new_payouts.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        let mut running_shells = previous_shells.unwrap_or(0);

        for payout in new_payouts {
            let shell_diff = payout.amount.parse::<i32>().map_err(|e| {
                JobError::Database(format!("Invalid payout amount '{}': {}", payout.amount, e))
            })?;

            let shells_then = running_shells;
            running_shells += shell_diff;

            let recorded_at = chrono::DateTime::parse_from_rfc3339(&payout.created_at)
                .map_err(|e| {
                    JobError::Database(format!(
                        "Invalid payout created_at '{}': {}",
                        payout.created_at, e
                    ))
                })?
                .with_timezone(&chrono::Utc);

            client
                .execute(
                    r#"
                INSERT INTO shell_history (slack_id, shells_then, shell_diff, shells, recorded_at)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT DO NOTHING
                "#,
                    &[
                        &slack_id,
                        &Some(shells_then),
                        &shell_diff,
                        &running_shells,
                        &recorded_at,
                    ],
                )
                .await
                .map_err(|e| JobError::Database(e.to_string()))?;
        }

        Ok(())
    }
}
