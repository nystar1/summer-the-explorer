use axum::{
    body::Body,
    http::Request,
    middleware::Next,
    response::Response,
};

pub async fn request_logger(
    req: Request<Body>,
    next: Next,
) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let start = std::time::Instant::now();

    tracing::info!(
        method = %method,
        uri = %uri,
        "Request started"
    );

    let response = next.run(req).await;
    let status = response.status();
    let duration = start.elapsed();

    tracing::info!(
        status = status.as_u16(),
        duration_ms = duration.as_millis(),
        "Request completed"
    );

    response
}