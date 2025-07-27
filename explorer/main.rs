mod utils;
mod models;
mod handlers;
mod services;

use std::sync::Arc;

use axum::{
    Json, Router,
    response::Html,
    routing::{get, post},
};
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use utoipa::OpenApi;
use utoipa_scalar::Scalar;
use tower_http::{cors::CorsLayer, services::ServeDir};

use common::utils::config::Config;
use common::database::connection::DbPool;

use utils::error::Result;
use services::embedding::EmbeddingService;
use handlers::{
    users::get_user_details,
    leaderboard::get_leaderboard,
    comments::{filter_comments, search_comments},
    logs::{filter_logs, get_log_details, search_logs},
    projects::{filter_projects, get_project_details, search_projects},
    mirror::{mirror_comments, mirror_devlogs, mirror_project, mirror_projects},
};

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub embedding_service: Arc<EmbeddingService>,
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Summer the Explorer",
        description = "API for exploring and analyzing Summer of Making projects. Not affiliated with Summer of Making or Hack Club.",
        version = "0.1.0",
        license(
            name = "MIT OR Apache-2.0",
            url = "https://opensource.org/licenses/MIT"
        ),
        contact(
            name = "Parth Ahuja"
        )
    ),
    paths(
        handlers::projects::search_projects,
        handlers::projects::filter_projects,
        handlers::projects::get_project_details,
        handlers::comments::search_comments,
        handlers::comments::filter_comments,
        handlers::logs::search_logs,
        handlers::logs::filter_logs,
        handlers::logs::get_log_details,
        handlers::users::get_user_details,
        handlers::leaderboard::get_leaderboard,
        handlers::mirror::mirror_projects,
        handlers::mirror::mirror_project,
        handlers::mirror::mirror_devlogs,
        handlers::mirror::mirror_comments,
    ),
    components(
        schemas(
            models::project::Project,
            models::project::ProjectFilter,
            models::project::ProjectSearchRequest,
            models::comment::Comment,
            models::comment::CommentFilter,
            models::comment::CommentSearchRequest,
            models::logs::Log,
            models::logs::LogFilter,
            models::logs::LogSearchRequest,
            models::user::User,
            models::user::UserFilter,
            models::user::LeaderboardEntry,
            models::user::LeaderboardResponse,
        )
    ),
    tags(
        (name = "projects", description = "Project management endpoints"),
        (name = "comments", description = "Comment management endpoints"),
        (name = "logs", description = "Devlog management endpoints"),
        (name = "users", description = "User management endpoints"),
        (name = "leaderboard", description = "Leaderboard endpoints"),
        (name = "mirror", description = "Mirror proxy endpoints"),
    )
)]
struct ApiDoc;

async fn serve_docs() -> Html<String> {
    Html(Scalar::new(ApiDoc::openapi()).to_html())
}

async fn serve_openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

fn create_router() -> Router<AppState> {
    Router::new()
        .route("/v1/projects/search", post(search_projects))
        .route("/v1/projects/filter", get(filter_projects))
        .route("/v1/projects/details", get(get_project_details))
        .route("/v1/comments/search", post(search_comments))
        .route("/v1/comments/filter", get(filter_comments))
        .route("/v1/devlogs/search", post(search_logs))
        .route("/v1/devlogs/filter", get(filter_logs))
        .route("/v1/devlogs/details", get(get_log_details))
        .route("/v1/users/details", get(get_user_details))
        .route("/v1/leaderboard", get(get_leaderboard))
        .route("/v1/mirror/projects", get(mirror_projects))
        .route("/v1/mirror/projects/{id}", get(mirror_project))
        .route("/v1/mirror/devlogs", get(mirror_devlogs))
        .route("/v1/mirror/comments", get(mirror_comments))
        .nest_service("/static", ServeDir::new("static"))
        .route("/api-docs/openapi.json", get(serve_openapi_json))
        .route("/v1/docs", get(serve_docs))
        .layer(ServiceBuilder::new().layer(CorsLayer::permissive()))
}

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install crypto provider");

    tracing_subscriber::fmt::init();

    let config = Config::from_env()?;

    let pool = common::database::connection::create_pool(&config).await?;

    let embedding_service =
        Arc::new(EmbeddingService::new(false)?);

    let app_state = AppState {
        pool,
        embedding_service,
    };

    let app = create_router().with_state(app_state);

    let addr = format!("0.0.0.0:{}", config.api_port);
    let listener = TcpListener::bind(&addr).await?;

    tracing::info!("Summer the Explorer starting on {}", addr);
    tracing::info!(
        "API documentation available at http://localhost:{}/v1/docs",
        config.api_port
    );

    axum::serve(listener, app).await?;

    Ok(())
}
