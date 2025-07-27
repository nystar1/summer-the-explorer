use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PaginationInfo {
    pub pages: Option<i32>,
    pub count: Option<i32>,
    pub page: Option<i32>,
    pub items: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub pagination: Option<PaginationInfo>,
}
#[derive(Debug, Deserialize)]
pub struct ProjectsResponse {
    pub projects: Vec<RawProject>,
    pub pagination: Option<PaginationInfo>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RawProject {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    pub readme_link: Option<String>,
    pub slack_id: String,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Deserialize)]
pub struct DevlogsResponse {
    pub devlogs: Vec<RawDevlog>,
    pub pagination: Option<PaginationInfo>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RawDevlog {
    pub id: i64,
    pub text: String,
    pub project_id: i64,
    pub slack_id: String,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Deserialize)]
pub struct CommentsResponse {
    pub comments: Vec<RawComment>,
    pub pagination: Option<PaginationInfo>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RawComment {
    pub text: String,
    pub devlog_id: i64,
    pub slack_id: String,
    pub created_at: String,
}
#[derive(Debug, Deserialize)]
pub struct HackatimeResponse {
    pub data: HackatimeData,
    pub trust_factor: TrustFactor,
}

#[derive(Debug, Deserialize)]
pub struct HackatimeData {
    pub username: Option<String>,
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
pub struct TrustFactor {
    pub trust_level: String,
    pub trust_value: i32,
}

#[derive(Debug, Deserialize)]
pub struct HackatimeRateLimitError {
    pub message: String,
    pub retry_after: u64,
}
#[derive(Debug, Deserialize)]
pub struct LeaderboardResponse {
    pub users: Vec<RawLeaderboardEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawLeaderboardEntry {
    pub slack_id: String,
    pub username: Option<String>,
    pub shells: i32,
    pub payouts: Option<Vec<RawPayout>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawPayout {
    pub id: String,
    pub amount: String,
    pub created_at: String,
    #[serde(rename = "type")]
    pub payout_type: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawUser {
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub trust_level: Option<String>,
    pub trust_value: Option<i32>,
}
