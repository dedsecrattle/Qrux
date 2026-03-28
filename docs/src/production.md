# Production operations

## Graceful shutdown

Send **SIGINT** (Ctrl+C) or **SIGTERM** (e.g. Kubernetes, systemd stop). Qrux will:

1. Stop accepting new QUIC connections and notify the metrics / HTTPS sidecars to exit their accept loops.
2. Wait for existing QUIC traffic to finish, up to **`limits.graceful_shutdown_secs`** (default 30s).
3. Exit.

If shutdown exceeds that window, the process still exits after the timeout (logged as a warning).

## Timeouts and limits

Tune **`[server.limits]`** in your TOML for your backends and SLAs:

- **`upstream_connect_timeout_secs`** — Fail fast if a TCP connect to an upstream hangs (default 10s).
- **`upstream_request_timeout_secs`** — Cap for connect + request + full response body from the upstream (default 120s). Must be ≥ connect timeout.
- **`max_request_body_bytes`** — Rejects oversized HTTP/3 request bodies with **413** (default 10 MiB).
- **`max_upstream_response_body_bytes`** — Protects memory if an upstream sends a huge body (default 50 MiB).
- **`max_idle_connections_per_upstream`** — TCP pool size per `host:port` (default 16).

When the upstream request timeout fires, the counter **`qrux_upstream_timeouts_total`** increments.

## Startup validation

The config must include at least one **`[[routes]]`** row with `upstream` or `upstreams`, and limit fields must be positive and consistent (see defaults in the [Configuration](configuration.md) table). Invalid configs fail at startup with a clear error.

## Observability

- **Logs:** `RUST_LOG=qrux=info` (or `debug`). Default filter in the binary includes `qrux=info`.
- **Metrics:** Prometheus scrape on `metrics_listen`; see [Metrics](metrics.md).

## Docker

The repository includes a `Dockerfile`, `docker-compose.yml`, and `docker/proxy.toml.example`. **Publish QUIC with UDP** (`-p 8443:8443/udp`). CI pushes to **GitHub Container Registry** (`ghcr.io/<owner>/qrux`); optionally configure **`DOCKERHUB_USERNAME`** (variable) and **`DOCKERHUB_TOKEN`** (secret) to push the same tags to **Docker Hub**. See the README for `docker build`, `docker compose`, `docker pull`, and manual `docker push`.
