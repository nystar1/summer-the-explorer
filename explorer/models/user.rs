use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct User {
    pub slack_id: String,
    pub username: Option<String>,
    pub trust_level: Option<String>,
    pub trust_value: Option<i32>,
    pub current_shells: Option<i32>,
    pub last_synced: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shell_history: Vec<ShellHistory>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<UserProject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pfp_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_24: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_32: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_48: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_72: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_192: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_512: Option<String>,
}

impl User {
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ShellHistory {
    pub id: i32,
    #[serde(rename = "shellsThen")]
    pub shells_then: Option<i32>,
    #[serde(rename = "shellDiff")]
    pub shell_diff: Option<i32>,
    pub shells: i32,
    pub recorded_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, IntoParams)]
pub struct UserFilter {
    #[serde(rename = "slackId")]
    pub slack_id: Option<String>,
    pub username: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserProject {
    pub id: i64,
    pub title: String,
}


#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Payout {
    pub id: String,
    pub amount: String,
    pub created_at: String,
    #[serde(rename = "type")]
    pub payout_type: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LeaderboardEntry {
    pub slack_id: String,
    pub username: Option<String>,
    pub shells: Option<i32>,
    pub rank: Option<i64>,
    pub payouts: Option<Vec<Payout>>,
    pub pfp_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_history: Option<Vec<ShellHistory>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LeaderboardResponse {
    pub entries: Vec<LeaderboardEntry>,
    pub total_count: i64,
    pub page: i32,
    pub per_page: i32,
}
