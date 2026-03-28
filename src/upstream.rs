use crate::config::Limits;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;

/// TCP connection pool for upstream servers
pub struct UpstreamPool {
    pools: Arc<Mutex<HashMap<String, Vec<TcpStream>>>>,
    max_idle_per_upstream: usize,
}

impl UpstreamPool {
    pub fn new(max_idle_per_upstream: usize) -> Self {
        UpstreamPool {
            pools: Arc::new(Mutex::new(HashMap::new())),
            max_idle_per_upstream,
        }
    }

    /// Get a connection to the upstream, reusing from pool if available
    pub async fn get(
        &self,
        upstream: &str,
        connect_timeout: std::time::Duration,
    ) -> Result<TcpStream> {
        {
            let mut pools = self.pools.lock().await;
            if let Some(conns) = pools.get_mut(upstream) {
                while let Some(conn) = conns.pop() {
                    let mut buf = [0u8; 1];
                    match conn.try_read(&mut buf) {
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            tracing::debug!(upstream = %upstream, "Reusing pooled connection");
                            return Ok(conn);
                        }
                        _ => {
                            tracing::debug!(upstream = %upstream, "Discarding dead pooled connection");
                            continue;
                        }
                    }
                }
            }
        }

        tracing::debug!(upstream = %upstream, "Opening new TCP connection");
        timeout(connect_timeout, TcpStream::connect(upstream))
            .await
            .map_err(|_| anyhow::anyhow!("upstream connect timed out"))?
            .with_context(|| format!("Failed to connect to upstream: {}", upstream))
    }

    /// Return a connection to the pool for reuse
    pub async fn put(&self, upstream: &str, conn: TcpStream) {
        let mut pools = self.pools.lock().await;
        let conns = pools.entry(upstream.to_string()).or_default();

        if conns.len() < self.max_idle_per_upstream {
            conns.push(conn);
            tracing::debug!(upstream = %upstream, pooled = conns.len(), "Connection returned to pool");
        } else {
            tracing::debug!(upstream = %upstream, "Pool full, dropping connection");
        }
    }
}

/// Forward an HTTP/1.1 request using connection pooling and production limits.
#[allow(clippy::too_many_arguments)]
pub async fn forward_request_pooled(
    pool: &UpstreamPool,
    upstream: &str,
    method: &str,
    path: &str,
    _host: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    limits: &Limits,
) -> Result<(u16, Vec<(String, String)>, Vec<u8>)> {
    let request_timeout = limits.upstream_request_timeout();
    let connect_timeout = limits.upstream_connect_timeout();
    let max_resp = limits.max_upstream_response_body_bytes;

    let fut = async {
        let mut stream = pool.get(upstream, connect_timeout).await?;

        let upstream_host = upstream.split(':').next().unwrap_or(upstream);

        let mut request = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\n",
            method, path, upstream_host
        );

        for (name, value) in headers {
            if name.starts_with(':') || name.eq_ignore_ascii_case("host") {
                continue;
            }
            request.push_str(&format!("{}: {}\r\n", name, value));
        }

        if let Some(body) = body {
            request.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }

        request.push_str("Connection: keep-alive\r\n\r\n");

        stream.write_all(request.as_bytes()).await?;
        if let Some(body) = body {
            stream.write_all(body).await?;
        }

        let result = read_http_response(&mut stream, max_resp).await;

        if result.is_ok() {
            pool.put(upstream, stream).await;
        }

        result
    };

    timeout(request_timeout, fut)
        .await
        .map_err(|_| anyhow::anyhow!("upstream request timed out"))?
}

async fn read_http_response(
    stream: &mut TcpStream,
    max_body: usize,
) -> Result<(u16, Vec<(String, String)>, Vec<u8>)> {
    let mut reader = BufReader::new(stream);

    let mut status_line = String::new();
    reader.read_line(&mut status_line).await?;
    let status_line = status_line.trim();

    let parts: Vec<&str> = status_line.splitn(3, ' ').collect();
    if parts.len() < 2 {
        anyhow::bail!("Invalid status line: {}", status_line);
    }
    let status_code: u16 = parts[1].parse().context("Invalid status code")?;

    let mut headers = Vec::new();
    let mut content_length: Option<usize> = None;
    let mut chunked = false;

    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let line = line.trim();

        if line.is_empty() {
            break;
        }

        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            let value = value.trim();

            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().ok();
            } else if name.eq_ignore_ascii_case("transfer-encoding")
                && value.eq_ignore_ascii_case("chunked")
            {
                chunked = true;
            }

            headers.push((name.to_string(), value.to_string()));
        }
    }

    let body = if chunked {
        read_chunked_body(&mut reader, max_body).await?
    } else if let Some(len) = content_length {
        if len > max_body {
            anyhow::bail!("upstream response body exceeds configured limit");
        }
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body).await?;
        body
    } else {
        Vec::new()
    };

    Ok((status_code, headers, body))
}

async fn read_chunked_body<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
    max_body: usize,
) -> Result<Vec<u8>> {
    let mut body = Vec::new();

    loop {
        let mut size_line = String::new();
        reader.read_line(&mut size_line).await?;
        let size_str = size_line.trim();

        let chunk_size = usize::from_str_radix(size_str, 16).context("Invalid chunk size")?;

        if chunk_size == 0 {
            let mut trailing = String::new();
            reader.read_line(&mut trailing).await?;
            break;
        }

        if body.len() + chunk_size > max_body {
            anyhow::bail!("upstream response body exceeds configured limit");
        }

        let mut chunk = vec![0u8; chunk_size];
        reader.read_exact(&mut chunk).await?;
        body.extend_from_slice(&chunk);

        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf).await?;
    }

    Ok(body)
}
