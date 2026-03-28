use crate::config::Limits;
use crate::metrics;
use crate::router::Router;
use crate::upstream::UpstreamPool;
use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

#[allow(clippy::too_many_arguments)]
pub async fn run(
    listen_addr: SocketAddr,
    quic_port: u16,
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    router: Arc<Router>,
    pool: Arc<UpstreamPool>,
    limits: Arc<Limits>,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) -> Result<()> {
    let mut rustls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("Failed to build TLS config for HTTPS fallback")?;

    rustls_config.alpn_protocols = vec![b"http/1.1".to_vec()];

    let tls_acceptor = TlsAcceptor::from(Arc::new(rustls_config));
    let listener = TcpListener::bind(listen_addr).await?;

    tracing::info!(
        addr = %listen_addr,
        quic_port = quic_port,
        "HTTPS fallback server listening (Alt-Svc enabled)"
    );

    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                tracing::info!(addr = %listen_addr, "HTTPS fallback server stopped");
                break;
            }
            accept = listener.accept() => {
                let (tcp_stream, remote_addr) = accept.context("HTTPS accept")?;
                let tls_acceptor = tls_acceptor.clone();
                let router = Arc::clone(&router);
                let pool = Arc::clone(&pool);
                let limits = Arc::clone(&limits);

                tokio::spawn(async move {
                    match tls_acceptor.accept(tcp_stream).await {
                        Ok(tls_stream) => {
                            let io = TokioIo::new(tls_stream);

                            let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                                let router = Arc::clone(&router);
                                let pool = Arc::clone(&pool);
                                let limits = Arc::clone(&limits);
                                async move {
                                    handle_request(req, router, pool, limits, quic_port, remote_addr).await
                                }
                            });

                            if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                                tracing::debug!(error = %e, "HTTPS connection error");
                            }
                        }
                        Err(e) => {
                            tracing::debug!(error = %e, "TLS handshake failed");
                        }
                    }
                });
            }
        }
    }
    Ok(())
}

async fn handle_request(
    req: Request<hyper::body::Incoming>,
    router: Arc<Router>,
    pool: Arc<UpstreamPool>,
    limits: Arc<Limits>,
    quic_port: u16,
    _remote_addr: SocketAddr,
) -> std::result::Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let method = req.method().as_str().to_string();
    let uri = req.uri().clone();
    let path = uri
        .path_and_query()
        .map(|pq: &http::uri::PathAndQuery| pq.as_str())
        .unwrap_or("/")
        .to_string();

    let host: Option<String> = req
        .headers()
        .get(http::header::HOST)
        .and_then(|h: &http::HeaderValue| h.to_str().ok())
        .map(|s: &str| s.to_string());

    tracing::info!(
        method = %method,
        path = %path,
        host = ?host,
        "Incoming HTTPS request (fallback)"
    );

    let upstream = match router.resolve(host.as_deref()) {
        Some(u) => u.to_string(),
        None => {
            return Ok(build_response(
                StatusCode::BAD_GATEWAY,
                "No upstream configured",
                quic_port,
            ));
        }
    };

    let headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|(k, v): (&http::HeaderName, &http::HeaderValue)| {
            (k.as_str().to_string(), v.to_str().unwrap_or("").to_string())
        })
        .collect();

    let start = Instant::now();

    match crate::upstream::forward_request_pooled(
        &pool,
        &upstream,
        &method,
        &path,
        host.as_deref().unwrap_or("localhost"),
        &headers,
        None,
        limits.as_ref(),
    )
    .await
    {
        Ok((status, resp_headers, resp_body)) => {
            let duration = start.elapsed().as_secs_f64();
            metrics::record_request(&method, status, &upstream, duration);

            let mut response =
                Response::builder().status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK));

            response = response.header(
                "alt-svc",
                format!(
                    "h3=\":{}\"; ma=86400, h3-29=\":{}\"; ma=86400",
                    quic_port, quic_port
                ),
            );

            for (name, value) in resp_headers {
                let lower = name.to_lowercase();
                if lower == "transfer-encoding" || lower == "connection" || lower == "keep-alive" {
                    continue;
                }
                response = response.header(name, value);
            }

            Ok(response.body(Full::new(Bytes::from(resp_body))).unwrap())
        }
        Err(e) => {
            let msg = format!("{e:#}");
            if msg.contains("timed out") {
                metrics::record_upstream_timeout();
            }
            tracing::error!(error = %e, "Upstream error");
            metrics::record_request(&method, 502, &upstream, start.elapsed().as_secs_f64());
            Ok(build_response(
                StatusCode::BAD_GATEWAY,
                &format!("Upstream error: {}", e),
                quic_port,
            ))
        }
    }
}

fn build_response(status: StatusCode, body: &str, quic_port: u16) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .header(
            "alt-svc",
            format!(
                "h3=\":{}\"; ma=86400, h3-29=\":{}\"; ma=86400",
                quic_port, quic_port
            ),
        )
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}
