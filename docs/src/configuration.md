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
