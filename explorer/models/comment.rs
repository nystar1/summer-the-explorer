use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Comment {
    pub id: i64,
    pub text: String,
    pub devlog_id: i64,
    pub slack_id: String,
    pub username: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_synced: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

impl Comment {
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = Some(confidence);
        self
    }

}


#[derive(Debug, Serialize, Deserialize, ToSchema, utoipa::IntoParams)]
pub struct CommentFilter {
    #[serde(rename = "projectId")]
    pub project_id: Option<i64>,
    #[serde(rename = "slackId")]
    pub slack_id: Option<String>,
    pub username: Option<String>,
    #[serde(rename = "devlogId")]
    pub devlog_id: Option<i64>,
    pub text: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
    #[serde(rename = "fromDate")]
    pub from_date: Option<String>,
    #[serde(rename = "toDate")]
    pub to_date: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CommentSearchRequest {
    pub query: String,
    pub limit: Option<u32>,
}
