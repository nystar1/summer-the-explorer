use crate::core::JobError;
use crate::trace::slack::SlackProfile;
use common::database::connection::DbPool;

pub struct UserUpdater;

impl UserUpdater {
    pub async fn find_users_needing_info(pool: &DbPool) -> Result<Vec<String>, JobError> {
        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let rows = client
            .query(
                "SELECT DISTINCT ON (slack_id) slack_id 
         FROM users 
         WHERE username IS NULL 
            OR pfp_url = 'notfound' 
            OR trust_level = 'unavailable'
         ORDER BY slack_id, last_synced ASC 
         LIMIT 100",
                &[],
            )
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    pub async fn update_user_with_slack_info(
        pool: &DbPool,
        slack_id: &str,
        username: &str,
        profile: &SlackProfile,
    ) -> Result<(), JobError> {
        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        let pfp_url = profile
            .image_192
            .as_ref()
            .or(profile.image_512.as_ref())
            .or(profile.image_72.as_ref())
            .or(profile.image_48.as_ref())
            .map(|s| s.as_str())
            .unwrap_or("notfound");

        client
            .execute(
                r#"UPDATE users SET 
                username = $1, 
                pfp_url = $2, 
                image_24 = $3,
                image_32 = $4,
                image_48 = $5,
                image_72 = $6,
                image_192 = $7,
                image_512 = $8,
                last_synced = NOW() 
                WHERE slack_id = $9"#,
                &[
                    &username,
                    &pfp_url,
                    &profile.image_24,
                    &profile.image_32,
                    &profile.image_48,
                    &profile.image_72,
                    &profile.image_192,
                    &profile.image_512,
                    &slack_id,
                ],
            )
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        Ok(())
    }

    pub async fn update_user_with_trust_info(
        pool: &DbPool,
        slack_id: &str,
        trust_level: &str,
        trust_value: i32,
    ) -> Result<(), JobError> {
        let client = pool
            .get()
            .await
            .map_err(|e| JobError::Database(e.to_string()))?;

        client.execute(
            "UPDATE users SET trust_level = $1, trust_value = $2, last_synced = NOW() WHERE slack_id = $3",
            &[&trust_level, &trust_value, &slack_id]
        ).await
        .map_err(|e| JobError::Database(e.to_string()))?;

        Ok(())
    }
}
