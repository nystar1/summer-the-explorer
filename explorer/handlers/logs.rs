use std::collections::HashMap;

use axum::Json;
use pgvector::Vector;
use axum::extract::{Query, State};

use crate::AppState;
use crate::utils::error::{ApiError, Result};
use crate::models::logs::{Log, LogFilter, LogSearchRequest};
use crate::utils::database::{decode_username, map_log_row, map_project_row, QueryBuilder};

#[utoipa::path(
    post,
    path = "/v1/devlogs/search",
    request_body = LogSearchRequest,
    responses(
        (status = 200, description = "Search results", body = [Log])
    ),
    tag = "logs"
)]
pub async fn search_logs(
    State(state): State<AppState>,
    Json(request): Json<LogSearchRequest>,
) -> Result<Json<Vec<Log>>> {
    let embedding_vec = state.embedding_service.embed_text(&request.query).await?;
    let embedding = Vector::from(embedding_vec);
    let limit = i64::from(request.limit.unwrap_or(20).min(100));

    let client = state.pool.get().await?;

    let rows = client
        .query(
            r#"
        SELECT 
            id, text, attachment, project_id, slack_id, username, 
            created_at, updated_at, last_synced,
            (1 - (text_embedding <=> $1)) as confidence
        FROM logs 
        WHERE text_embedding IS NOT NULL
        ORDER BY text_embedding <=> $1
        LIMIT $2
        "#,
            &[&embedding, &limit],
        )
        .await?;

    let logs: Vec<Log> = rows
        .iter()
        .map(|row| {
            let confidence: f64 = row.get("confidence");
            map_log_row(row).with_confidence(confidence)
        })
        .collect();

    Ok(Json(logs))
}

#[utoipa::path(
    get,
    path = "/v1/devlogs/filter",
    params(LogFilter),
    responses(
        (status = 200, description = "Filtered logs", body = [Log])
    ),
    tag = "logs"
)]
pub async fn filter_logs(
    State(state): State<AppState>,
    Query(filter): Query<LogFilter>,
) -> Result<Json<Vec<Log>>> {
    let client = state.pool.get().await?;
    let mut query_builder = QueryBuilder::new();

    if let Some(project_id) = filter.project_id {
        query_builder.add_condition("project_id = ${}", project_id);
    }

    if let Some(slack_id) = filter.slack_id {
        query_builder.add_condition("slack_id = ${}", slack_id);
    }

    if let Some(username) = filter.username {
        let decoded = decode_username(&username);
        query_builder.add_condition("username ILIKE ${}", decoded);
    }

    if let Some(text) = filter.text {
        query_builder.add_condition("text ILIKE ${}", text);
    }

    query_builder.add_date_range_condition(
        "created_at", 
        filter.from_date.as_deref(), 
        filter.to_date.as_deref()
    )?;

    let limit = i64::from(filter.limit.unwrap_or(20).min(100));
    query_builder.add_condition("1=1", limit);

    let where_clause = query_builder.build_where_clause();
    let params = query_builder.params();
    let param_count = query_builder.param_count();

    let query = format!(
        r#"
        SELECT 
            id, text, attachment, project_id, slack_id, username, 
            created_at, updated_at, last_synced
        FROM logs 
        {}
        ORDER BY created_at DESC 
        LIMIT ${}
        "#,
        where_clause,
        param_count
    );

    let rows = client.query(&query, &params).await?;
    let logs: Vec<Log> = rows.iter().map(map_log_row).collect();

    Ok(Json(logs))
}


#[utoipa::path(
    get,
    path = "/v1/devlogs/details",
    params(
        ("id" = i64, Query, description = "Log ID")
    ),
    responses(
        (status = 200, description = "Log details", body = Log),
        (status = 404, description = "Log not found")
    ),
    tag = "logs"
)]
pub async fn get_log_details(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Log>> {
    let log_id = params
        .get("id")
        .ok_or_else(|| ApiError::Validation {
            field: "id".to_string(),
            message: "Missing log ID".to_string(),
        })?
        .parse::<i64>()
        .map_err(|_| ApiError::Validation {
            field: "id".to_string(),
            message: "Invalid log ID".to_string(),
        })?;

    let client = state.pool.get().await?;
    
    let log_rows = client
        .query(
            r#"
        SELECT 
            id, text, attachment, project_id, slack_id, username, 
            created_at, updated_at, last_synced
        FROM logs 
        WHERE id = $1
        "#,
            &[&log_id],
        )
        .await?;

    let log_row = log_rows.first().ok_or_else(|| ApiError::NotFound {
        resource: "Log".to_string(),
        id: log_id.to_string(),
    })?;

    let log = map_log_row(log_row);

    let project_rows = client
        .query(
            r#"
        SELECT 
            id, title, description, category, readme_link, demo_link, 
            repo_link, slack_id, username, created_at, updated_at,
            last_synced
        FROM projects 
        WHERE id = $1
        "#,
            &[&log.project_id],
        )
        .await?;

    let log_with_project = if let Some(project_row) = project_rows.first() {
        let project = map_project_row(project_row);
        log.with_project(project)
    } else {
        log
    };

    Ok(Json(log_with_project))
}
