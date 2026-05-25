use std::net::SocketAddr;
use std::time::Duration;

use axum::{Router, routing::get};
use middleware::auth::AuthLayer;
use middleware::rate_limit::RateLimitLayer;
use tower::ServiceBuilder;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

mod routes;
mod tls;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = middleware::telemetry::init_telemetry();

    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret".into());
    let openai_key = std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| "sk-placeholder".into());
    let openai_url = std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com".into());
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| "sk-ant-placeholder".into());
    let anthropic_url = std::env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| "https://api.anthropic.com".into());

    let state = routes::AppState::with_defaults(openai_key, openai_url, anthropic_key, anthropic_url);

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().level(Level::INFO).include_headers(false))
        .on_response(DefaultOnResponse::new().level(Level::INFO).latency_unit(tower_http::LatencyUnit::Micros));

    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .merge(routes::router(state))
        .layer(
            ServiceBuilder::new()
                .layer(trace_layer)
                .layer(RateLimitLayer::new(100, Duration::from_secs(1)))
                .layer(AuthLayer::jwt(jwt_secret)),
        );

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    match (option_env!("TLS_CERT"), option_env!("TLS_KEY")) {
        (Some(cert), Some(key)) => {
            let acceptor = tls::load_tls_acceptor(cert, key)?;
            tls::serve_tls(app, addr, acceptor).await?;
        }
        _ => {
            tracing::info!("listening on {addr} (plaintext)");
            let listener = tokio::net::TcpListener::bind(addr).await?;
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}
