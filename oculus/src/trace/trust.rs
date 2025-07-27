use crate::core::JobError;
use common::services::external::ExternalApiService;

pub struct TrustManager;

impl TrustManager {
    pub async fn fetch_trust_info(
        external_api: &ExternalApiService,
        slack_id: &str,
    ) -> Result<Option<(String, i32)>, JobError> {
        match external_api.fetch_user_stats(slack_id).await {
            Ok(Some(stats)) => Ok(Some((
                stats.trust_factor.trust_level,
                stats.trust_factor.trust_value,
            ))),
            Ok(None) => Ok(None),
            Err(_) => Ok(None),
        }
    }
}
