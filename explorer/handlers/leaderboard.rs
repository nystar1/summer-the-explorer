use axum::{
    Json,
    extract::{Query, State},
};
use std::collections::HashMap;

use crate::{
    AppState,
    models::user::{
        LeaderboardEntry, LeaderboardResponse, ShellHistory,
    },
    utils::error::Result,
};

#[utoipa::path(
    get,
    path = "/v1/leaderboard",
    params(
        ("pullAll" = Option<bool>, Query, description = "Pull all entries"),
        ("historicalData" = Option<bool>, Query, description = "Include historical data and payouts"),
        ("page" = Option<i32>, Query, description = "Page number"),
        ("per_page" = Option<i32>, Query, description = "Items per page")
    ),
    responses(
        (status = 200, description = "Leaderboard", body = LeaderboardResponse)
    ),
    tag = "leaderboard"
)]
#[allow(clippy::too_many_lines, clippy::items_after_statements)] // what one must do for clippy
pub async fn get_leaderboard(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<LeaderboardResponse>> {
    let pull_all = params.get("pullAll").is_some_and(|v| v == "true");

    let historical_data = params
        .get("historicalData")
        .is_some_and(|v| v == "true");
    let page = params.get("page").and_then(|p| p.parse().ok()).unwrap_or(1);
    let per_page = if pull_all {
        1_000_000
    } else {
        params
            .get("per_page")
            .and_then(|p| p.parse().ok())
            .unwrap_or(50)
            .min(100)
    };

    let offset = (page - 1) * per_page;

    let client = state.pool.get().await?;
    let count_row = client.query_one("SELECT COUNT(*) FROM users WHERE current_shells > 0", &[]).await?;
    let total_count: i64 = count_row.get(0);

    let mut entries: Vec<LeaderboardEntry> = if historical_data {
        let rows = client
            .query(
                r#"
            SELECT 
                u.slack_id,
                u.username,
                u.pfp_url,
                COALESCE(MAX(sh.shells), u.current_shells) as shells,
                RANK() OVER (ORDER BY COALESCE(MAX(sh.shells), u.current_shells) DESC) as rank
            FROM users u
            LEFT JOIN shell_history sh ON u.slack_id = sh.slack_id
            WHERE u.current_shells > 0 OR sh.shells > 0
            GROUP BY u.slack_id, u.username, u.current_shells, u.pfp_url
            ORDER BY shells DESC
            LIMIT $1 OFFSET $2
            "#,
                &[&i64::from(per_page), &i64::from(offset)],
            )
            .await?;

        rows.into_iter()
            .map(|row| LeaderboardEntry {
                slack_id: row.get("slack_id"),
                username: row.get("username"),
                shells: row.get("shells"),
                rank: row.get("rank"),
                payouts: None,
                pfp_url: row.get("pfp_url"),
                shell_history: None,
            })
            .collect()
    } else {
        let rows = client
            .query(
                r#"
            SELECT 
                slack_id,
                username,
                pfp_url,
                current_shells as shells,
                RANK() OVER (ORDER BY current_shells DESC) as rank
            FROM users
            WHERE current_shells > 0
            ORDER BY current_shells DESC
            LIMIT $1 OFFSET $2
            "#,
                &[&i64::from(per_page), &i64::from(offset)],
            )
            .await?;

        rows.into_iter()
            .map(|row| LeaderboardEntry {
                slack_id: row.get("slack_id"),
                username: row.get("username"),
                shells: row.get("shells"),
                rank: row.get("rank"),
                payouts: None,
                pfp_url: row.get("pfp_url"),
                shell_history: None,
            })
            .collect()
    };

    if historical_data {
        let slack_ids: Vec<&String> = entries.iter().map(|e| &e.slack_id).collect();

        if !slack_ids.is_empty() {
            let all_histories = client.query(
                "SELECT id, slack_id, shells_then, shell_diff, shells, recorded_at FROM shell_history 
                 WHERE slack_id = ANY($1) 
                 ORDER BY slack_id, recorded_at ASC",
                &[&slack_ids]
            ).await?;

            let mut histories_by_slack_id: HashMap<String, Vec<ShellHistory>> =
                HashMap::with_capacity(slack_ids.len());

            for row in all_histories {
                let hist = ShellHistory {
                    id: row.get("id"),
                    shells_then: row.get("shells_then"),
                    shell_diff: row.get("shell_diff"),
                    shells: row.get("shells"),
                    recorded_at: row.get("recorded_at"),
                };
                histories_by_slack_id
                    .entry(row.get("slack_id"))
                    .or_default()
                    .push(hist);
            }

            for entry in &mut entries {
                if let Some(hist) = histories_by_slack_id.get(&entry.slack_id) {
                    entry.shell_history = Some(hist.clone());
                }
            }
        }
    }

    Ok(Json(LeaderboardResponse {
        entries,
        total_count,
        page,
        per_page,
    }))
}

