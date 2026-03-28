use crate::metrics;
use crate::router::Router;
use crate::upstream::{self, UpstreamPool};
use anyhow::{Context, Result};
use bytes::{Buf, Bytes};
use h3::server::RequestStream;
use http::{Request, Response, StatusCode};
use std::sync::Arc;
use std::time::Instant;

pub async fn handle_connection(
    connection: quinn::Connection,
    router: Arc<Router>,
    pool: Arc<UpstreamPool>,
    sni: Option<String>,
) -> Result<()> {
    let h3_conn = h3::server::Connection::new(h3_quinn::Connection::new(connection))
        .await
        .context("Failed to establish H3 connection")?;

    handle_h3_connection(h3_conn, router, pool, sni).await
}

async fn handle_h3_connection(
    mut conn: h3::server::Connection<h3_quinn::Connection, Bytes>,
    router: Arc<Router>,
    pool: Arc<UpstreamPool>,
    sni: Option<String>,
) -> Result<()> {
    loop {
        match conn.accept().await {
            Ok(Some(resolver)) => {
                let router = Arc::clone(&router);
                let pool = Arc::clone(&pool);
                let sni = sni.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_resolver(resolver, router, pool, sni).await {
                        tracing::error!(error = %e, "Request handling error");
                    }
                });
            }
            Ok(None) => {
                tracing::debug!("Connection closed gracefully");
                break;
            }
            Err(e) => {
                // Timeout and connection close are normal for idle connections
                let msg = e.to_string();
                if msg.contains("Timeout") || msg.contains("closed") {
                    tracing::debug!(error = %e, "Connection closed (idle timeout or client disconnect)");
                } else {
                    tracing::error!(error = %e, "Connection error");
                }
                break;
            }
        }
    }

    Ok(())
}

async fn handle_resolver(
    resolver: h3::server::RequestResolver<h3_quinn::Connection, Bytes>,
    router: Arc<Router>,
    pool: Arc<UpstreamPool>,
    sni: Option<String>,
) -> Result<()> {
    // Resolve the request - this can fail if the client disconnects mid-handshake
    let (req, stream) = match resolver.resolve_request().await {
        Ok(r) => r,
        Err(e) => {
            // Client likely disconnected (e.g., cert rejection) - not a real error
            tracing::debug!(error = %e, "Client disconnected before request completed");
            return Ok(());
        }
    };
    handle_request(req, stream, router, pool, sni).await
}

async fn handle_request(
    req: Request<()>,
    mut stream: RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
    router: Arc<Router>,
    pool: Arc<UpstreamPool>,
    sni: Option<String>,
) -> Result<()> {
    let method = req.method().as_str();
    let path = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let authority = req
        .uri()
        .authority()
        .map(|a| a.as_str())
        .or_else(|| req.headers().get("host").and_then(|h| h.to_str().ok()));

    tracing::info!(
        method = %method,
        path = %path,
        authority = ?authority,
        "Incoming HTTP/3 request"
    );

    // Resolve upstream using SNI first, then authority
    let hostname = sni.as_deref().or(authority);
    let upstream = match router.resolve(hostname) {
        Some(u) => u,
        None => {
            tracing::warn!(hostname = ?hostname, "No upstream configured");
            send_error_response(
                &mut stream,
                StatusCode::BAD_GATEWAY,
                "No upstream configured",
            )
            .await?;
            return Ok(());
        }
    };

    tracing::debug!(upstream = %upstream, "Forwarding to upstream");

    // Collect headers
    let headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    // Read request body if present
    let body = read_request_body(&mut stream).await?;
    let body_ref = if body.is_empty() {
        None
    } else {
        Some(body.as_slice())
    };

    // Forward to upstream using connection pool
    let host = authority.unwrap_or("localhost");
    let start = Instant::now();

    let (status, result) = match upstream::forward_request_pooled(
        &pool, upstream, method, path, host, &headers, body_ref,
    )
    .await
    {
        Ok((status, resp_headers, resp_body)) => {
            send_response(&mut stream, status, &resp_headers, &resp_body).await?;
            (status, Ok(()))
        }
        Err(e) => {
            tracing::error!(error = %e, "Upstream error");
            send_error_response(&mut stream, StatusCode::BAD_GATEWAY, &e.to_string()).await?;
            (502, Err(e))
        }
    };

    // Record metrics
    let duration = start.elapsed().as_secs_f64();
    metrics::record_request(method, status, upstream, duration);

    result
}

async fn read_request_body(
    stream: &mut RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = stream.recv_data().await? {
        body.extend_from_slice(chunk.chunk());
    }
    Ok(body)
}

async fn send_response(
    stream: &mut RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<()> {
    let mut response = Response::builder().status(StatusCode::from_u16(status)?);

    for (name, value) in headers {
        // Skip hop-by-hop headers
        let lower = name.to_lowercase();
        if lower == "transfer-encoding" || lower == "connection" || lower == "keep-alive" {
            continue;
        }
        response = response.header(name.as_str(), value.as_str());
    }

    let response = response.body(()).context("Failed to build response")?;
    stream.send_response(response).await?;

    if !body.is_empty() {
        stream.send_data(Bytes::copy_from_slice(body)).await?;
    }

    stream.finish().await?;

    Ok(())
}

async fn send_error_response(
    stream: &mut RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
    status: StatusCode,
    message: &str,
) -> Result<()> {
    let response = Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(())
        .context("Failed to build error response")?;

    stream.send_response(response).await?;
    stream
        .send_data(Bytes::copy_from_slice(message.as_bytes()))
        .await?;
    stream.finish().await?;

    Ok(())
}
