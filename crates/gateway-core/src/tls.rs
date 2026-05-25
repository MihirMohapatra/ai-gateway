use std::sync::Arc;

use axum::Router;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::server::conn::{http1, http2};
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower::Service;

async fn body_to_axum(
    req: http::Request<Incoming>,
) -> Result<http::Request<axum::body::Body>, hyper::Error> {
    let (parts, body) = req.into_parts();
    let bytes = body.collect().await?.to_bytes();
    Ok(http::Request::from_parts(parts, axum::body::Body::from(bytes)))
}

pub fn load_tls_acceptor(cert_path: &str, key_path: &str) -> anyhow::Result<TlsAcceptor> {
    let certs = rustls_pemfile::certs(&mut std::io::BufReader::new(
        std::fs::File::open(cert_path)?,
    ))
    .collect::<Result<Vec<_>, _>>()?;

    let key = rustls_pemfile::private_key(&mut std::io::BufReader::new(
        std::fs::File::open(key_path)?,
    ))?
    .ok_or_else(|| anyhow::anyhow!("no private key found in {}", key_path))?;

    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub async fn serve_tls(
    app: Router,
    addr: std::net::SocketAddr,
    acceptor: TlsAcceptor,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("listening on {addr} (TLS)");

    loop {
        let (stream, peer) = listener.accept().await?;
        let app = app.clone();
        let acceptor = acceptor.clone();

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(%peer, "tls handshake failed: {e}");
                    return;
                }
            };

            let alpn = tls_stream.get_ref().1.alpn_protocol().map(|v| v.to_vec());
            let io = TokioIo::new(tls_stream);

            let svc = service_fn(move |req: http::Request<Incoming>| {
                let mut app = app.clone();
                async move {
                    let req = body_to_axum(req).await?;
                    let res = app.call(req).await.unwrap();
                    Ok::<_, anyhow::Error>(res)
                }
            });

            let result = match alpn.as_deref() {
                Some(b"h2") => {
                    http2::Builder::new(TokioExecutor::new())
                        .serve_connection(io, svc)
                        .await
                }
                _ => {
                    http1::Builder::new()
                        .serve_connection(io, svc)
                        .await
                }
            };

            if let Err(e) = result {
                tracing::warn!(%peer, "connection error: {e}");
            }
        });
    }
}
