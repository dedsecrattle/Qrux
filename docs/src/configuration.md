# Configuration

Configuration is TOML, passed with `--config <file>` (see the example `proxy.toml` in the repo).

## Server block

| Field | Description |
|-------|-------------|
| `listen` | QUIC / HTTP/3 listen address (e.g. `0.0.0.0:8443`) |
| `cert` | Path to TLS certificate (PEM) |
| `key` | Path to TLS private key (PEM) |
| `metrics_listen` | Optional. If set, exposes Prometheus metrics on this address |
| `https_listen` | Optional. If set, listens for HTTPS (TCP/TLS) and adds `Alt-Svc` for HTTP/3 discovery |

### Optional: `[server.limits]`

Omitted keys use **defaults** (safe for typical deployments). See [Production](production.md).

| Field | Default | Description |
|-------|---------|-------------|
| `upstream_connect_timeout_secs` | `10` | TCP connect timeout to upstream |
| `upstream_request_timeout_secs` | `120` | End-to-end timeout per upstream request |
| `max_request_body_bytes` | `10485760` (10 MiB) | Max HTTP/3 request body from clients |
| `max_upstream_response_body_bytes` | `52428800` (50 MiB) | Max body read from upstream |
| `max_idle_connections_per_upstream` | `16` | Pooled idle TCP connections per upstream |
| `graceful_shutdown_secs` | `30` | Max wait after SIGINT/SIGTERM for QUIC to drain |

Example:

```toml
[server.limits]
upstream_connect_timeout_secs = 5
upstream_request_timeout_secs = 60
max_request_body_bytes = 2097152
```

## Routes

Each `[[routes]]` entry maps hostnames to upstreams:

- **`match`** — Hostname from TLS SNI or HTTP `:authority` / `Host`. Use `"*"` as a catch-all.
- **`upstream`** — Single upstream `host:port`.
- **`upstreams`** — List of `host:port` values; requests are distributed **round-robin**.

Use either `upstream` or `upstreams`, not both for the same route.

### Example

```toml
[server]
listen = "0.0.0.0:8443"
cert = "./certs/cert.pem"
key = "./certs/key.pem"
metrics_listen = "127.0.0.1:9090"
https_listen = "0.0.0.0:8444"

[[routes]]
match = "api.example.com"
upstream = "127.0.0.1:8080"

[[routes]]
match = "app.example.com"
upstreams = [
  "server1.internal:8080",
  "server2.internal:8080",
  "server3.internal:8080",
]

[[routes]]
match = "*"
upstream = "127.0.0.1:8080"
```
