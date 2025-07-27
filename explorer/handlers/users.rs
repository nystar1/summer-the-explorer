use axum::{
    Json,
    extract::{Query, State},
};

use crate::{
    AppState,
    models::user::{ShellHistory, User, UserFilter, UserProject},
    utils::{database::decode_username, error::{ApiError, Result}},
};

#[utoipa::path(
    get,
    path = "/v1/users/details",
    params(UserFilter),
    responses(
        (status = 200, description = "User details", body = User),
        (status = 404, description = "User not found")
    ),
    tag = "users"
)]
#[allow(clippy::too_many_lines, clippy::items_after_statements)]
pub async fn get_user_details(
    State(state): State<AppState>,
    Query(filter): Query<UserFilter>,
) -> Result<Json<User>> {
    let client = state.pool.get().await?;

    let mut conditions = Vec::with_capacity(2);
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::with_capacity(2);
    let mut param_count = 0;
    
    if let Some(slack_id) = &filter.slack_id {
        param_count += 1;
        conditions.push(format!("u.slack_id = ${}", param_count));
        params.push(slack_id);
    }

    let decoded_username;
    if let Some(username) = &filter.username {
        decoded_username = decode_username(username);
        param_count += 1;
        conditions.push(format!("u.username ILIKE ${}", param_count));
        params.push(&decoded_username);
    }

    if conditions.is_empty() {
        return Err(ApiError::Validation {
            field: "filter".to_string(),
            message: "Must provide at least one filter parameter".to_string(),
        });
    }

    let limit = i64::from(filter.limit.unwrap_or(20).min(100));
    param_count += 1;
    params.push(&limit);

    let query = format!(
        r#"
        SELECT 
            u.slack_id, u.username, u.trust_level, u.trust_value,
            u.current_shells, u.last_synced, u.pfp_url,
            u.image_24, u.image_32, u.image_48, u.image_72, 
            u.image_192, u.image_512,
            sh.id, sh.shells_then, sh.shell_diff, sh.shells, sh.recorded_at
        FROM users u
        LEFT JOIN shell_history sh ON u.slack_id = sh.slack_id
        WHERE {}
        ORDER BY sh.recorded_at DESC
        LIMIT ${}
        "#,
        conditions.join(" AND "),
        param_count
    );

    let statement = client.prepare(&query).await?;
    let rows = client.query(&statement, &params[..]).await?;

    if rows.is_empty() {
        return Err(ApiError::NotFound {
            resource: "User".to_owned(),
            id: "unknown".to_owned(),
        });
    }

    let first_row = &rows[0];
    let user = User {
        slack_id: first_row.get("slack_id"),
        username: first_row.get("username"),
        trust_level: first_row.get("trust_level"),
        trust_value: first_row.get("trust_value"),
        current_shells: first_row.get("current_shells"),
        last_synced: first_row.get("last_synced"),
        shell_history: Vec::new(),
        projects: Vec::new(),
        pfp_url: first_row.get("pfp_url"),
        image_24: first_row.get("image_24"),
        image_32: first_row.get("image_32"),
        image_48: first_row.get("image_48"),
        image_72: first_row.get("image_72"),
        image_192: first_row.get("image_192"),
        image_512: first_row.get("image_512"),
    };

    let shell_history: Vec<ShellHistory> = rows
        .iter()
        .filter_map(|row| {
            row.get::<_, Option<i32>>("id").map(|id| ShellHistory {
                id,
                shells_then: row.get("shells_then"),
                shell_diff: row.get("shell_diff"),
                shells: row.get("shells"),
                recorded_at: row.get("recorded_at"),
            })
        })
        .collect();

    let project_statement = client.prepare(
        "SELECT id, title FROM projects WHERE slack_id = $1 ORDER BY created_at DESC"
    ).await?;
    let project_rows = client.query(&project_statement, &[&user.slack_id]).await?;

    let projects: Vec<UserProject> = project_rows
        .iter()
        .map(|row| UserProject {
            id: row.get("id"),
            title: row.get("title"),
        })
        .collect();

    Ok(Json(User {
        slack_id: user.slack_id,
        username: user.username,
        trust_level: user.trust_level,
        trust_value: user.trust_value,
        current_shells: user.current_shells,
        last_synced: user.last_synced,
        shell_history,
        projects,
        pfp_url: user.pfp_url,
        image_24: user.image_24,
        image_32: user.image_32,
        image_48: user.image_48,
        image_72: user.image_72,
        image_192: user.image_192,
        image_512: user.image_512,
    }))
}
