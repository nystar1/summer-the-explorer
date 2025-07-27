use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::collections::HashMap;

use crate::AppState;
use crate::models::{comment::Comment, logs::Log, project::Project};
use crate::utils::error::Result;

struct PaginationParams {
    page: i32,
    per_page: i32,
    offset: i32,
}

fn extract_pagination(params: &HashMap<String, String>) -> PaginationParams {
    let page = params.get("page").and_then(|p| p.parse().ok()).unwrap_or(1);
    let per_page = 20;
    let offset = (page - 1) * per_page;
    
    PaginationParams { page, per_page, offset }
}

fn validate_pagination(page: i32, total: i64, per_page: i32) -> bool {
    page >= 1 && (i64::from(page) <= (total + i64::from(per_page) - 1) / i64::from(per_page) || total == 0)
}

#[utoipa::path(
    get,
    path = "/v1/mirror/projects",
    params(
        ("page" = Option<i32>, Query, description = "Page number")
    ),
    responses(
        (status = 200, description = "Mirrored projects", body = [Project])
    ),
    tag = "mirror"
)]
pub async fn mirror_projects(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>> {
    let pagination = extract_pagination(&params);
    let client = state.pool.get().await?;
    
    let total_row = client
        .query_one("SELECT COUNT(*) FROM projects", &[])
        .await?;
    let total: i64 = total_row.get(0);
    let total_pages = (total + i64::from(pagination.per_page) - 1) / i64::from(pagination.per_page);

    if !validate_pagination(pagination.page, total, pagination.per_page) {
        return Ok(Json(json!({"error": "Page out of bounds"})));
    }

    let project_rows = client
        .query(
            r#"
        SELECT 
            id, title, description, category, readme_link, demo_link, 
            repo_link, slack_id, username, created_at, updated_at, last_synced
        FROM projects 
        ORDER BY created_at DESC
        LIMIT $1 OFFSET $2
        "#,
            &[&i64::from(pagination.per_page), &i64::from(pagination.offset)],
        )
        .await?;

    let projects: Vec<Project> = project_rows
        .into_iter()
        .map(|row| Project {
            id: row.get::<_, i64>("id"),
            title: row.get("title"),
            description: row.get("description"),
            category: row.get("category"),
            readme_link: row.get("readme_link"),
            demo_link: row.get("demo_link"),
            repo_link: row.get("repo_link"),
            slack_id: row.get("slack_id"),
            username: row.get("username"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            last_synced: row.get("last_synced"),
            confidence: None,
            comments: Vec::new(),
        })
        .collect();

    Ok(Json(json!({
        "projects": projects,
        "pagination": {
            "page": pagination.page,
            "pages": total_pages,
            "count": total,
            "items": pagination.per_page
        }
    })))
}

#[utoipa::path(
    get,
    path = "/v1/mirror/projects/{id}",
    params(
        ("id" = i64, Path, description = "Project ID")
    ),
    responses(
        (status = 200, description = "Mirrored project", body = Project)
    ),
    tag = "mirror"
)]
pub async fn mirror_project(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>> {
    let client = state.pool.get().await?;
    let project_rows = client
        .query(
            r#"
        SELECT 
            id, title, description, category, readme_link, demo_link, 
            repo_link, slack_id, created_at, updated_at
        FROM projects 
        WHERE id = $1
        "#,
            &[&id],
        )
        .await?;

    if let Some(row) = project_rows.first() {
        Ok(Json(json!({
            "id": row.get::<_, i64>("id"),
            "title": row.get::<_, String>("title"),
            "description": row.get::<_, Option<String>>("description"),
            "category": row.get::<_, Option<String>>("category"),
            "readme_link": row.get::<_, Option<String>>("readme_link"),
            "demo_link": row.get::<_, Option<String>>("demo_link"),
            "repo_link": row.get::<_, Option<String>>("repo_link"),
            "slack_id": row.get::<_, String>("slack_id"),
            "created_at": row.get::<_, DateTime<Utc>>("created_at"),
            "updated_at": row.get::<_, DateTime<Utc>>("updated_at")
        })))
    } else {
        Err(crate::utils::error::ApiError::NotFound {
            resource: "Project".to_string(),
            id: id.to_string(),
        })
    }
}

#[utoipa::path(
    get,
    path = "/v1/mirror/devlogs",
    params(
        ("page" = Option<i32>, Query, description = "Page number")
    ),
    responses(
        (status = 200, description = "Mirrored devlogs", body = [Log])
    ),
    tag = "mirror"
)]
pub async fn mirror_devlogs(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>> {
    let pagination = extract_pagination(&params);
    let client = state.pool.get().await?;
    
    let total_row = client.query_one("SELECT COUNT(*) FROM logs", &[]).await?;
    let total: i64 = total_row.get(0);
    let total_pages = (total + i64::from(pagination.per_page) - 1) / i64::from(pagination.per_page);

    if !validate_pagination(pagination.page, total, pagination.per_page) {
        return Ok(Json(json!({"error": "Page out of bounds"})));
    }

    let devlog_rows = client
        .query(
            r#"
        SELECT 
            id, text, attachment, project_id, slack_id, username, 
            created_at, updated_at, last_synced
        FROM logs 
        ORDER BY created_at DESC
        LIMIT $1 OFFSET $2
        "#,
            &[&i64::from(pagination.per_page), &i64::from(pagination.offset)],
        )
        .await?;

    let devlogs: Vec<Log> = devlog_rows
        .into_iter()
        .map(|row| Log {
            id: row.get::<_, i64>("id"),
            text: row.get("text"),
            attachment: row.get("attachment"),
            project_id: row.get::<_, i64>("project_id"),
            slack_id: row.get("slack_id"),
            username: row.get("username"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            last_synced: row.get("last_synced"),
            confidence: None,
            project: None,
        })
        .collect();

    Ok(Json(json!({
        "devlogs": devlogs,
        "pagination": {
            "page": pagination.page,
            "pages": total_pages,
            "count": total,
            "items": pagination.per_page
        }
    })))
}

#[utoipa::path(
    get,
    path = "/v1/mirror/comments",
    params(
        ("page" = Option<i32>, Query, description = "Page number")
    ),
    responses(
        (status = 200, description = "Mirrored comments", body = [Comment])
    ),
    tag = "mirror"
)]
pub async fn mirror_comments(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>> {
    let pagination = extract_pagination(&params);
    let client = state.pool.get().await?;
    
    let total_row = client
        .query_one("SELECT COUNT(*) FROM comments", &[])
        .await?;
    let total: i64 = total_row.get(0);
    let total_pages = (total + i64::from(pagination.per_page) - 1) / i64::from(pagination.per_page);

    if !validate_pagination(pagination.page, total, pagination.per_page) {
        return Ok(Json(json!({"error": "Page out of bounds"})));
    }

    let comment_rows = client
        .query(
            r#"
        SELECT 
            id, text, devlog_id, slack_id, username, created_at,
            last_synced
        FROM comments 
        ORDER BY created_at DESC
        LIMIT $1 OFFSET $2
        "#,
            &[&i64::from(pagination.per_page), &i64::from(pagination.offset)],
        )
        .await?;

    let comments: Vec<Comment> = comment_rows
        .into_iter()
        .map(|row| Comment {
            id: row.get::<_, i64>("id"),
            text: row.get("text"),
            devlog_id: row.get::<_, i64>("devlog_id"),
            slack_id: row.get("slack_id"),
            username: row.get("username"),
            created_at: row.get("created_at"),
            last_synced: row.get("last_synced"),
            confidence: None,
        })
        .collect();

    Ok(Json(json!({
        "comments": comments,
        "pagination": {
            "page": pagination.page,
            "pages": total_pages,
            "count": total,
            "items": pagination.per_page
        }
    })))
}
