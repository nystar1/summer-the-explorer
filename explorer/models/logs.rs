use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Log {
    pub id: i64,
    pub text: String,
    pub attachment: Option<String>,
    pub project_id: i64,
    pub slack_id: String,
    pub username: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_synced: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<crate::models::project::Project>,
}

impl Log {
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = Some(confidence);
        self
    }

    pub fn with_project(mut self, project: crate::models::project::Project) -> Self {
        self.project = Some(project);
        self
    }

}


#[derive(Debug, Serialize, Deserialize, ToSchema, utoipa::IntoParams)]
pub struct LogFilter {
    #[serde(rename = "projectId")]
    pub project_id: Option<i64>,
    #[serde(rename = "slackId")]
    pub slack_id: Option<String>,
    pub username: Option<String>,
    #[serde(rename = "devlogId")]
    pub devlog_id: Option<i64>,
    pub text: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(rename = "fromDate")]
    pub from_date: Option<String>,
    #[serde(rename = "toDate")]
    pub to_date: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LogSearchRequest {
    pub query: String,
    pub limit: Option<u32>,
}
