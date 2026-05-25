use axum::{Router, routing::get};
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

mod request;
mod routes;
mod tls;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .merge(routes::router());

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
