# Prometheus metrics

When `metrics_listen` is set, **Qrux** serves Prometheus text format at **`/metrics`** on that address.

Metric names use the **`qrux_` prefix**.

Example:

```text
http://127.0.0.1:9090/metrics
```

## Typical metrics

**Request counts** (labels: method, status, upstream):

```text
qrux_http_requests_total{method="GET",status="200",upstream="example.com:80"} 42
```

**Latency** (histogram, labels: method, upstream):

```text
qrux_http_request_duration_seconds_bucket{method="GET",upstream="example.com:80",le="0.1"} 40
```

**Active QUIC connections:**

```text
qrux_active_connections 5
```

**Pooled upstream TCP connections** (label: upstream):

```text
qrux_upstream_pool_connections{upstream="example.com:80"} 3
```

Scrape this endpoint with Prometheus or inspect it with `curl` while debugging.
