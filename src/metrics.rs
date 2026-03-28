use anyhow::Result;
use lazy_static::lazy_static;
use prometheus::{
    register_counter_vec, register_gauge, register_histogram_vec, CounterVec, Encoder, Gauge,
    HistogramVec, TextEncoder,
};
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

lazy_static! {
    pub static ref HTTP_REQUESTS_TOTAL: CounterVec = register_counter_vec!(
        "qrux_http_requests_total",
        "Total number of HTTP requests",
        &["method", "status", "upstream"]
    )
    .unwrap();
    pub static ref HTTP_REQUEST_DURATION_SECONDS: HistogramVec = register_histogram_vec!(
        "qrux_http_request_duration_seconds",
        "HTTP request duration in seconds",
        &["method", "upstream"],
        vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    )
    .unwrap();
    pub static ref ACTIVE_CONNECTIONS: Gauge = register_gauge!(
        "qrux_active_connections",
        "Number of active QUIC connections"
    )
    .unwrap();
    pub static ref UPSTREAM_POOL_SIZE: prometheus::GaugeVec = prometheus::register_gauge_vec!(
        "qrux_upstream_pool_connections",
        "Number of pooled connections per upstream",
        &["upstream"]
    )
    .unwrap();
}

pub fn gather_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

pub fn record_request(method: &str, status: u16, upstream: &str, duration_secs: f64) {
    HTTP_REQUESTS_TOTAL
        .with_label_values(&[method, &status.to_string(), upstream])
        .inc();

    HTTP_REQUEST_DURATION_SECONDS
        .with_label_values(&[method, upstream])
        .observe(duration_secs);
}

pub fn inc_connections() {
    ACTIVE_CONNECTIONS.inc();
}

pub fn dec_connections() {
    ACTIVE_CONNECTIONS.dec();
}

pub fn set_pool_size(upstream: &str, size: usize) {
    UPSTREAM_POOL_SIZE
        .with_label_values(&[upstream])
        .set(size as f64);
}

pub async fn serve_metrics(addr: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(addr = %addr, "Metrics server listening");

    loop {
        let (mut socket, _) = listener.accept().await?;

        tokio::spawn(async move {
            let metrics = gather_metrics();
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                Content-Type: text/plain; version=0.0.4; charset=utf-8\r\n\
                Content-Length: {}\r\n\
                \r\n\
                {}",
                metrics.len(),
                metrics
            );

            let _ = socket.write_all(response.as_bytes()).await;
        });
    }
}
