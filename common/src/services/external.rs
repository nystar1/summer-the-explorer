use crate::utils::error::{ApiError, Result};
use crate::utils::modal::{
    CommentsResponse, DevlogsResponse, HackatimeRateLimitError, HackatimeResponse,
    LeaderboardResponse, ProjectsResponse, RawLeaderboardEntry,
};

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use reqwest::{Client, ClientBuilder, cookie::Jar};

#[derive(Clone)]
pub struct ExternalApiService {
    client: Client,
    journey_session_cookie: String,
}

impl ExternalApiService {
    pub fn new(journey_session_cookie: String) -> Result<Self> {
        let jar = Arc::new(Jar::default());

        let client = ClientBuilder::new()
            .cookie_provider(Arc::clone(&jar))
            .user_agent("summer-the-explorer/1.0")
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| ApiError::ExternalApi(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            journey_session_cookie,
        })
    }

    pub async fn fetch_projects(&self, page: Option<i32>) -> Result<ProjectsResponse> {
        let mut url = "https://summer.hackclub.com/api/v1/projects".to_string();
        if let Some(page) = page {
            url.push_str(&format!("?page={}", page));
        }
        self.fetch_with_retry(&url).await
    }

    pub async fn fetch_devlogs(&self, page: Option<i32>) -> Result<DevlogsResponse> {
        let mut url = "https://summer.hackclub.com/api/v1/devlogs".to_string();
        if let Some(page) = page {
            url.push_str(&format!("?page={}", page));
        }
        self.fetch_with_retry(&url).await
    }

    pub async fn fetch_comments(&self, page: Option<i32>) -> Result<CommentsResponse> {
        let mut url = "https://summer.hackclub.com/api/v1/comments".to_string();
        if let Some(page) = page {
            url.push_str(&format!("?page={}", page));
        }
        self.fetch_with_retry(&url).await
    }

    pub async fn fetch_leaderboard(&self) -> Result<LeaderboardResponse> {
        let url = "https://explorpheus.hackclub.com/leaderboard?historicalData=true";
        let users: Vec<RawLeaderboardEntry> = self.fetch_with_retry(url).await?;
        Ok(LeaderboardResponse { users })
    }

    pub async fn fetch_user_stats(&self, slack_id: &str) -> Result<Option<HackatimeResponse>> {
        let url = format!(
            "https://hackatime.hackclub.com/api/v1/users/{}/stats",
            slack_id
        );
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ApiError::ExternalApi(format!("Failed to fetch user stats: {}", e)))?;
        if response.status() == 404 {
            return Ok(None);
        }
        if response.status() == 429 {
            let response_text = response.text().await.map_err(|e| {
                ApiError::ExternalApi(format!("Failed to read rate limit response: {}", e))
            })?;
            let rate_limit_error: HackatimeRateLimitError = serde_json::from_str(&response_text)
                .map_err(|e| {
                    ApiError::ExternalApi(format!("Failed to parse rate limit response: {}", e))
                })?;
            return Err(ApiError::RateLimit {
                retry_after: rate_limit_error.retry_after,
                message: rate_limit_error.message,
            });
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read response body".to_string());
            return Err(ApiError::ExternalApi(format!(
                "Hackatime stats API returned status {}: {}",
                status, body
            )));
        }
        let stats_response = response.json::<HackatimeResponse>().await.map_err(|e| {
            ApiError::ExternalApi(format!("Failed to parse hackatime stats response: {}", e))
        })?;
        Ok(Some(stats_response))
    }

    async fn fetch_with_retry<T>(&self, url: &str) -> Result<T>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        let mut backoff_ms = 1000;
        
        for attempt in 1..=5 {
            let response = self
                .client
                .get(url)
                .header("Cookie", format!("_journey_session={}", self.journey_session_cookie))
                .timeout(Duration::from_secs(30))
                .send()
                .await;
                
            match response {
                Ok(response) => {
                    if let Some(retry_after) = response.headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        sleep(Duration::from_secs(retry_after)).await;
                        continue;
                    }
                    
                    let status = response.status();
                    if !status.is_success() {
                        let body = response.text().await
                            .unwrap_or_else(|_| "Unable to read response body".to_string());
                            
                        return match status.as_u16() {
                            403 if body.contains("get blocked nerd") => Err(ApiError::ExternalApi(
                                format!("Access blocked by API (403): {}. You may need to access from a different IP or wait before retrying.", body)
                            )),
                            403 => Err(ApiError::ExternalApi(
                                format!("Authentication failed (403). The session cookie may have expired. Status: {}, Body: {}", status, body)
                            )),
                            429 | 500..=599 if attempt < 5 => {
                                let delay = Duration::from_millis(backoff_ms);
                                tracing::warn!("Error {}, retrying in {:?} (attempt {}/5)", status, delay, attempt);
                                sleep(delay).await;
                                backoff_ms = (backoff_ms * 2).min(30_000);
                                continue;
                            }
                            _ => Err(ApiError::ExternalApi(format!("HTTP error: {} - {}", status, body)))
                        };
                    }
                    
                    let response_text = response.text().await
                        .map_err(|e| ApiError::ExternalApi(format!("Failed to read response body: {}", e)))?;
                    return serde_json::from_str(&response_text)
                        .map_err(|e| ApiError::ExternalApi(format!("Failed to parse API response: {}", e)));
                }
                Err(e) if attempt < 5 && (e.is_timeout() || e.is_connect()) => {
                    let delay = Duration::from_millis(backoff_ms);
                    tracing::warn!("Network error {}, retrying in {:?} (attempt {}/5)", e, delay, attempt);
                    sleep(delay).await;
                    backoff_ms = (backoff_ms * 2).min(30_000);
                }
                Err(e) => return Err(ApiError::ExternalApi(format!("Failed to fetch from {}: {}", url, e)))
            }
        }
        
        unreachable!()
    }
}

