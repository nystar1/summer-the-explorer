use chrono::{DateTime, NaiveDate, Utc};
use serde_urlencoded;
use std::collections::HashMap;
use tokio_postgres::{types::ToSql, Row};

use super::error::{ApiError, Result};
use crate::models::{comment::Comment, logs::Log, project::Project};

pub fn parse_date_string(date_str: &str) -> Result<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(date_str) {
        return Ok(dt.with_timezone(&Utc));
    }

    if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        return Ok(DateTime::from_naive_utc_and_offset(
            date.and_hms_opt(0, 0, 0).expect("Valid time components"),
            Utc,
        ));
    }

    let parse_with_format = |fmt: &str| {
        NaiveDate::parse_from_str(date_str, fmt)
            .map(|d| d.and_hms_opt(0, 0, 0).expect("Valid time components"))
            .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
    };

    parse_with_format("%d/%m/%Y")
        .or_else(|_| parse_with_format("%d-%m-%Y"))
        .map_err(|_| ApiError::Validation {
            field: "date".to_string(),
            message: format!(
                "Invalid date format: {}. Expected ISO 8601 or DD/MM/YYYY",
                date_str
            ),
        })
}

pub fn decode_username(username: &str) -> String {
    let query_string = format!("username={}", username);
    serde_urlencoded::from_str::<HashMap<String, String>>(&query_string)
        .ok()
        .and_then(|parsed| parsed.get("username").cloned())
        .unwrap_or_else(|| username.to_string())
}

pub struct QueryBuilder {
    conditions: Vec<String>,
    params: Vec<Box<dyn ToSql + Send + Sync>>,
}

impl QueryBuilder {
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
            params: Vec::new(),
        }
    }

    pub fn add_condition<T: ToSql + Send + Sync + 'static>(&mut self, condition: &str, value: T) {
        let param_index = self.params.len() + 1;
        self.conditions.push(condition.replace("{}", &param_index.to_string()));
        self.params.push(Box::new(value));
    }

    pub fn add_date_condition(&mut self, field: &str, operator: &str, date_str: &str) -> Result<()> {
        let parsed = parse_date_string(date_str)?;
        self.add_condition(&format!("{} {} ${}", field, operator, "{}"), parsed);
        Ok(())
    }

    pub fn add_date_range_condition(&mut self, field: &str, from_date: Option<&str>, to_date: Option<&str>) -> Result<()> {
        if let Some(from) = from_date {
            self.add_date_condition(field, ">=", from)?;
        }
        if let Some(to) = to_date {
            self.add_date_condition(field, "<=", to)?;
        }
        Ok(())
    }

    pub fn build_where_clause(&self) -> String {
        if self.conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.conditions.join(" AND "))
        }
    }

    pub fn params(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params.iter().map(|p| p.as_ref() as &(dyn ToSql + Sync)).collect()
    }

    pub fn param_count(&self) -> usize {
        self.params.len()
    }
}

impl Default for QueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn map_project_row(row: &Row) -> Project {
    Project {
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
    }
}

pub fn map_comment_row(row: &Row) -> Comment {
    Comment {
        id: row.get::<_, i64>("id"),
        text: row.get("text"),
        devlog_id: row.get::<_, i64>("devlog_id"),
        slack_id: row.get("slack_id"),
        username: row.get("username"),
        created_at: row.get("created_at"),
        last_synced: row.get("last_synced"),
        confidence: None,
    }
}

pub fn map_log_row(row: &Row) -> Log {
    Log {
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
    }
}

