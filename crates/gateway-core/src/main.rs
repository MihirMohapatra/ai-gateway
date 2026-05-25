use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use config::loader::{load, watch};
use middleware::auth::AuthLayer;
use middleware::rate_limit::RateLimitLayer;
use tower::ServiceBuilder;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

mod metrics;
mod routes;
mod tls;

fn config_path() -> String {
    std::env::var("GATEWAY_CONFIG").unwrap_or_else(|_| "gateway.toml".into())
}

async fn hot_reload_loop(state: Arc<routes::AppState>, watcher: config::loader::ConfigWatcher) {
    let mut rx = watcher.rx;
    while rx.changed().await.is_ok() {
        let config = rx.borrow().clone();
        tracing::info!("config changed, reloading routing rules");
        state.reload_config(config);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = middleware::telemetry::init_telemetry();

    let config_path = config_path();
    let config = load(&config_path)?;

    let state = routes::AppState::with_defaults(&config);

    if let Ok(watcher) = watch(&config_path) {
        let hb_state = Arc::clone(&state);
        tokio::spawn(async move { hot_reload_loop(hb_state, watcher).await });
        tracing::info!("watching {config_path} for changes");
    } else {
        tracing::warn!("file watching not available, hot reload disabled");
    }

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(
            DefaultMakeSpan::new()
                .level(Level::INFO)
                .include_headers(false),
        )
        .on_response(
            DefaultOnResponse::new()
                .level(Level::INFO)
                .latency_unit(tower_http::LatencyUnit::Micros),
        );

    let jwt_secret = config.auth.jwt_secret.clone();
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse().unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], 3000)));

    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "OK" }))
        .merge(routes::router(state))
        .layer(
            ServiceBuilder::new()
                .layer(trace_layer)
                .layer(RateLimitLayer::new(100, Duration::from_secs(1)))
                .layer(AuthLayer::jwt(jwt_secret)),
        );

    match (
        config.server.tls_cert_path.as_ref(),
        config.server.tls_key_path.as_ref(),
    ) {
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
