use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Project {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub readme_link: Option<String>,
    pub demo_link: Option<String>,
    pub repo_link: Option<String>,
    pub slack_id: String,
    pub username: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_synced: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<crate::models::comment::Comment>,
}

impl Project {
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = Some(confidence);
        self
    }

    pub fn with_comments(mut self, comments: Vec<crate::models::comment::Comment>) -> Self {
        self.comments = comments;
        self
    }

}


#[derive(Debug, Serialize, Deserialize, ToSchema, utoipa::IntoParams)]
pub struct ProjectFilter {
    pub id: Option<i64>,
    #[serde(rename = "slackId")]
    pub slack_id: Option<String>,
    pub username: Option<String>,
    pub title: Option<String>,
    pub category: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
    #[serde(rename = "fromDate")]
    pub from_date: Option<String>,
    #[serde(rename = "toDate")]
    pub to_date: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProjectSearchRequest {
    pub query: String,
    pub limit: Option<u32>,
}
