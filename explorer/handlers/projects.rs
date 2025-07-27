use std::collections::HashMap;

use axum::Json;
use pgvector::Vector;
use axum::extract::{Query, State};

use crate::AppState;
use crate::utils::error::{ApiError, Result};
use crate::models::project::{Project, ProjectFilter, ProjectSearchRequest};
use crate::utils::database::{decode_username, map_comment_row, map_project_row, QueryBuilder};

#[utoipa::path(
    post,
    path = "/v1/projects/search",
    request_body = ProjectSearchRequest,
    responses(
        (status = 200, description = "Search results", body = [Project])
    ),
    tag = "projects"
)]
pub async fn search_projects(
    State(state): State<AppState>,
    Json(request): Json<ProjectSearchRequest>,
) -> Result<Json<Vec<Project>>> {
    let embedding_vec = state.embedding_service.embed_text(&request.query).await?;
    let embedding = Vector::from(embedding_vec);
    let limit = i64::from(request.limit.unwrap_or(20).min(100));

    let client = state.pool.get().await?;

    let rows = client
        .query(
            r#"
        SELECT 
            id, title, description, category, readme_link, demo_link, 
            repo_link, slack_id, username, created_at, updated_at, last_synced,
            (1 - (title_description_embedding <=> $1)) as confidence
        FROM projects 
        WHERE title_description_embedding IS NOT NULL
        ORDER BY title_description_embedding <=> $1
        LIMIT $2
        "#,
            &[&embedding, &limit],
        )
        .await?;

    let projects: Vec<Project> = rows
        .iter()
        .map(|row| {
            let confidence: f64 = row.get("confidence");
            map_project_row(row).with_confidence(confidence)
        })
        .collect();

    Ok(Json(projects))
}

#[utoipa::path(
    get,
    path = "/v1/projects/filter",
    params(ProjectFilter),
    responses(
        (status = 200, description = "Filtered projects", body = [Project])
    ),
    tag = "projects"
)]
pub async fn filter_projects(
    State(state): State<AppState>,
    Query(filter): Query<ProjectFilter>,
) -> Result<Json<Vec<Project>>> {
    let client = state.pool.get().await?;
    let mut query_builder = QueryBuilder::new();

    if let Some(id) = filter.id {
        query_builder.add_condition("id = ${}", id);
    }

    if let Some(slack_id) = filter.slack_id {
        query_builder.add_condition("slack_id = ${}", slack_id);
    }

    if let Some(username) = filter.username {
        let decoded = decode_username(&username);
        query_builder.add_condition("username ILIKE ${}", decoded);
    }

    if let Some(title) = filter.title {
        query_builder.add_condition("title ILIKE ${}", title);
    }

    if let Some(category) = filter.category {
        query_builder.add_condition("category = ${}", category);
    }

    if let Some(created_at_str) = filter.created_at.as_deref() {
        query_builder.add_date_condition("created_at", "=", created_at_str)?;
    }

    if let Some(updated_at_str) = filter.updated_at.as_deref() {
        query_builder.add_date_condition("updated_at", "=", updated_at_str)?;
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
        "SELECT id, title, description, category, readme_link, demo_link, 
         repo_link, slack_id, username, created_at, updated_at, last_synced 
         FROM projects 
         {} 
         ORDER BY updated_at DESC 
         LIMIT ${}", 
        where_clause,
        param_count
    );

    let rows = client.query(&query, &params).await?;
    let projects = rows.iter().map(map_project_row).collect();

    Ok(Json(projects))
}


#[utoipa::path(
    get,
    path = "/v1/projects/details",
    params(
        ("id" = i64, Query, description = "Project ID")
    ),
    responses(
        (status = 200, description = "Project details", body = Project),
        (status = 404, description = "Project not found")
    ),
    tag = "projects"
)]
pub async fn get_project_details(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Project>> {
    let project_id = params
        .get("id")
        .ok_or_else(|| ApiError::Validation {
            field: "id".to_string(),
            message: "Missing project ID".to_string(),
        })?
        .parse::<i64>()
        .map_err(|_| ApiError::Validation {
            field: "id".to_string(),
            message: "Invalid project ID".to_string(),
        })?;

    let client = state.pool.get().await?;

    let project_rows = client
        .query(
            r#"
        SELECT 
            id, title, description, category, readme_link, demo_link, 
            repo_link, slack_id, username, created_at, updated_at, last_synced
        FROM projects 
        WHERE id = $1
        "#,
            &[&project_id],
        )
        .await?;

    let project_row = project_rows.first().ok_or_else(|| ApiError::NotFound {
        resource: "Project".to_string(),
        id: project_id.to_string(),
    })?;

    let project = map_project_row(project_row);

    let comment_rows = client
        .query(
            r#"
        SELECT 
            c.id, c.text, c.devlog_id, c.slack_id, c.username, 
            c.created_at, c.last_synced
        FROM comments c
        JOIN logs l ON c.devlog_id = l.id
        WHERE l.project_id = $1
        ORDER BY c.created_at DESC
        "#,
            &[&project_id],
        )
        .await?;

    let comments = comment_rows.iter().map(map_comment_row).collect();

    Ok(Json(project.with_comments(comments)))
}
