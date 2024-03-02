use axum::http::header;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;

async fn health() -> &'static str {
    "OK"
}

async fn metrics() -> Response {
    if let Ok(body) = crate::metrics::metrics().await {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/openmetrics-text; version=1.0.0; charset=utf-8"),
        );
        (headers, body).into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to generate metrics",
        )
            .into_response()
    }
}

pub async fn server() {
    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics));

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Could not bind to metrics port");
    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!("Encountered error serving api: {e}");
    }
}
