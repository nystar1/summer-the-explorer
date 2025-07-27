use crate::core::JobError;
use common::utils::config::Config;
use parking_lot::RwLock;
use serde::Deserialize;
use std::time::Instant;

#[derive(Debug, Deserialize)]
pub struct SlackProfile {
    pub display_name: Option<String>,
    pub real_name: Option<String>,
    pub image_24: Option<String>,
    pub image_32: Option<String>,
    pub image_48: Option<String>,
    pub image_72: Option<String>,
    pub image_192: Option<String>,
    pub image_512: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SlackProfileResponse {
    ok: bool,
    profile: Option<SlackProfile>,
}

pub struct SlackManager {
    config: Config,
    slack_token: RwLock<Option<(String, Instant)>>,
}

impl SlackManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            slack_token: RwLock::new(None),
        }
    }

    pub fn get_slack_token(&self) -> Result<String, JobError> {
        if !self.config.slack_token.is_empty() {
            return Ok(self.config.slack_token.clone());
        }

        {
            let read = self.slack_token.read();
            if let Some((tok, t)) = &*read {
                if t.elapsed().as_secs() < 36000 {
                    return Ok(tok.clone());
                }
            }
        }

        {
            let read = self.slack_token.read();
            if let Some((tok, t)) = &*read {
                if t.elapsed().as_secs() < 36000 {
                    return Ok(tok.clone());
                }
            }
        }

        Err(JobError::ExternalApi(
            "Slack API requires a bot token. Please set SLACK_TOKEN environment variable with your bot token (xoxb-...)".to_string()
        ))
    }

    pub async fn fetch_user_info_from_slack(
        &self,
        slack_id: &str,
    ) -> Result<Option<(String, SlackProfile)>, JobError> {
        let client = reqwest::Client::new();
        let profile_url = format!("https://slack.com/api/users.profile.get?user={}", slack_id);

        let response = client
            .get(&profile_url)
            .header(
                "Authorization",
                format!("Bearer {}", self.get_slack_token()?),
            )
            .send()
            .await;

        match response {
            Ok(resp) => {
                if resp.status() == 429 {
                    if let Some(retry_after) = resp.headers().get("retry-after") {
                        if let Ok(retry_str) = retry_after.to_str() {
                            if let Ok(retry_seconds) = retry_str.parse::<u64>() {
                                tokio::time::sleep(tokio::time::Duration::from_secs(retry_seconds))
                                    .await;
                                return Err(JobError::Other("rate_limited".to_string()));
                            }
                        }
                    }
                    return Err(JobError::Other("rate_limited".to_string()));
                }

                if resp.status().is_success() {
                    match resp.json::<SlackProfileResponse>().await {
                        Ok(profile_response) => {
                            if profile_response.ok {
                                if let Some(profile) = profile_response.profile {
                                    let username = profile
                                        .display_name
                                        .clone()
                                        .or_else(|| profile.real_name.clone())
                                        .unwrap_or_else(|| "unknown".to_string());
                                    return Ok(Some((username, profile)));
                                }
                            }
                            Ok(Some((
                                "unknown".to_string(),
                                SlackProfile {
                                    display_name: None,
                                    real_name: None,
                                    image_24: None,
                                    image_32: None,
                                    image_48: None,
                                    image_72: None,
                                    image_192: None,
                                    image_512: None,
                                },
                            )))
                        }
                        Err(_e) => Ok(Some((
                            "unknown".to_string(),
                            SlackProfile {
                                display_name: None,
                                real_name: None,
                                image_24: None,
                                image_32: None,
                                image_48: None,
                                image_72: None,
                                image_192: None,
                                image_512: None,
                            },
                        ))),
                    }
                } else {
                    Ok(None)
                }
            }
            Err(_e) => Ok(None),
        }
    }
}
