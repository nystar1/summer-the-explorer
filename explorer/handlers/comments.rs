use axum::Json;
use pgvector::Vector;
use tracing::{info, instrument};
use axum::extract::{Query, State};

use crate::AppState;
use crate::utils::error::Result;
use crate::utils::database::{decode_username, map_comment_row, QueryBuilder};
use crate::models::comment::{Comment, CommentFilter, CommentSearchRequest};

#[utoipa::path(
    post,
    path = "/v1/comments/search",
    request_body = CommentSearchRequest,
    responses(
        (status = 200, description = "Search results", body = [Comment])
    ),
    tag = "comments"
)]
#[instrument(skip(state), fields(query = %request.query, limit = request.limit.unwrap_or(20)))]
pub async fn search_comments(
    State(state): State<AppState>,
    Json(request): Json<CommentSearchRequest>,
) -> Result<Json<Vec<Comment>>> {
    let embedding_vec = state.embedding_service.embed_text(&request.query).await?;
    let embedding = Vector::from(embedding_vec);
    let limit = i64::from(request.limit.unwrap_or(20).min(100));

    let client = state.pool.get().await?;

    let rows = client
        .query(
            r#"
            SELECT 
                id, text, devlog_id, slack_id, username, created_at, last_synced,
                (1 - (text_embedding <=> $1)) as confidence
            FROM comments 
            WHERE text_embedding IS NOT NULL
            ORDER BY text_embedding <=> $1
            LIMIT $2
            "#,
            &[&embedding, &limit],
        )
        .await?;

    let comments = rows
        .into_iter()
        .map(|row| {
            let confidence: f64 = row.get("confidence");
            map_comment_row(&row).with_confidence(confidence)
        })
        .collect();

    Ok(Json(comments))
}

#[utoipa::path(
    get,
    path = "/v1/comments/filter",
    params(CommentFilter),
    responses(
        (status = 200, description = "Filtered comments", body = [Comment])
    ),
    tag = "comments"
)]
#[instrument(skip(state), fields(devlog_id = ?filter.devlog_id, slack_id = ?filter.slack_id, has_text_filter = filter.text.is_some()))]
pub async fn filter_comments(
    State(state): State<AppState>,
    Query(filter): Query<CommentFilter>,
) -> Result<Json<Vec<Comment>>> {
    let client = state.pool.get().await?;
    let mut query_builder = QueryBuilder::new();

    if let Some(devlog_id) = filter.devlog_id {
        query_builder.add_condition("devlog_id = ${}", devlog_id);
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
        "SELECT id, text, devlog_id, slack_id, username, created_at, last_synced 
         FROM comments 
         {} 
         ORDER BY created_at DESC 
         LIMIT ${}", 
        where_clause,
        param_count
    );

    let rows = client.query(&query, &params).await?;
    let comments: Vec<_> = rows.iter().map(map_comment_row).collect();

    info!(
        results_count = comments.len(),
        "Filter completed successfully"
    );

    Ok(Json(comments))
}

