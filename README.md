# Qrux

**Qrux** is a QUIC/HTTP3-terminating proxy that forwards traffic to plain TCP/HTTP backends.

**Repository:** [github.com/dedsecrattle/Qrux](https://github.com/dedsecrattle/Qrux)

**Documentation:** [dedsecrattle.github.io/Qrux](https://dedsecrattle.github.io/Qrux/) — built from `docs/` with mdBook via [GitHub Pages](https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site) (**GitHub Actions** source). If the site is empty, enable Pages and run the “Deploy docs” workflow once.

```
Client (HTTP/3 over QUIC) ──→ [Qrux] ──→ Backend (HTTP/1.1 over TCP)
```

## Features

- **QUIC with TLS 1.3** (via quinn + rustls)
- **HTTP/3 protocol support**
- **SNI/Host-based routing** to multiple backends
- **Round-robin load balancing** across multiple upstreams
- **Connection pooling** for upstream TCP connections
- **Prometheus metrics** endpoint
- **HTTPS fallback** with Alt-Svc header for browser HTTP/3 discovery
- **0-RTT support** for returning clients

## Quick Start

### 1. Generate TLS certificates

```bash
# Using mkcert (recommended for local dev)
brew install mkcert
mkcert -install
cd certs && mkcert -key-file key.pem -cert-file cert.pem localhost 127.0.0.1 ::1

# Or using the provided script
./scripts/gen-certs.sh
```

### 2. Create config file

Create `proxy.toml`:

```toml
[server]
listen = "0.0.0.0:8443"           # QUIC/HTTP3 port
cert = "./certs/cert.pem"
key = "./certs/key.pem"
metrics_listen = "127.0.0.1:9090" # Prometheus metrics
https_listen = "0.0.0.0:8444"     # HTTPS fallback (Alt-Svc)

[[routes]]
match = "*"
upstream = "example.com:80"

# Or with load balancing:
# upstreams = ["server1:80", "server2:80", "server3:80"]
```

### 3. Run the proxy

```bash
cargo run --release -- --config proxy.toml
```

### 4. Test with curl

```bash
# HTTP/3 (requires curl with quiche)
/opt/homebrew/opt/curl/bin/curl --http3-only -k https://localhost:8443/

# Or via HTTPS fallback (any curl)
curl -k https://localhost:8444/
```

## Configuration

```toml
[server]
listen = "0.0.0.0:8443"           # QUIC listen address
cert = "./certs/cert.pem"         # TLS certificate
key = "./certs/key.pem"           # TLS private key
metrics_listen = "127.0.0.1:9090" # Optional: Prometheus endpoint
https_listen = "0.0.0.0:8444"     # Optional: HTTPS fallback with Alt-Svc

# Routes matched by TLS SNI or HTTP Host header
[[routes]]
match = "api.example.com"
upstream = "127.0.0.1:8080"       # Single upstream

[[routes]]
match = "app.example.com"
upstreams = [                      # Multiple upstreams (round-robin)
  "server1.internal:8080",
  "server2.internal:8080",
  "server3.internal:8080"
]

[[routes]]
match = "*"                        # Wildcard catches all
upstream = "127.0.0.1:8080"
```

## Prometheus Metrics

Available at `http://127.0.0.1:9090/metrics`. Names use the `quicproxy_` prefix (crate name).

```
# Total requests by method, status, and upstream
quicproxy_http_requests_total{method="GET",status="200",upstream="example.com:80"} 42

# Request latency histogram
quicproxy_http_request_duration_seconds_bucket{method="GET",upstream="example.com:80",le="0.1"} 40

# Active QUIC connections
quicproxy_active_connections 5

# Pooled upstream connections
quicproxy_upstream_pool_connections{upstream="example.com:80"} 3
```

## Browser Support (Alt-Svc)

The HTTPS fallback server (`https_listen`) sends `Alt-Svc` headers to advertise HTTP/3:

```
Alt-Svc: h3=":8443"; ma=86400
```

Browsers like Chrome will:
1. First connect via HTTPS (port 8444)
2. See the Alt-Svc header
3. Upgrade to HTTP/3 (port 8443) for subsequent requests

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                       Qrux                          │
│                                                     │
│  ┌──────────────┐  ┌──────────────┐                 │
│  │ QUIC/HTTP3   │  │ HTTPS        │  ◄── Alt-Svc   │
│  │ :8443        │  │ :8444        │      header    │
│  └──────┬───────┘  └──────┬───────┘                 │
│         │                 │                         │
│         └────────┬────────┘                         │
│                  ▼                                  │
│         ┌────────────────┐                          │
│         │     Router     │  ◄── SNI/Host matching  │
│         │  (round-robin) │                          │
│         └────────┬───────┘                          │
│                  ▼                                  │
│         ┌────────────────┐                          │
│         │  Upstream Pool │  ◄── Connection reuse   │
│         │    (TCP)       │                          │
│         └────────┬───────┘                          │
│                  ▼                                  │
│         ┌────────────────┐                          │
│         │    Backends    │                          │
│         └────────────────┘                          │
└─────────────────────────────────────────────────────┘
```

## 0-RTT Security Considerations

0-RTT (early data) enables faster connections but data can be replayed.

**Safe for:** GET requests, read-only operations

**Unsafe for:** POST/PUT/DELETE, financial transactions

See [Cloudflare's 0-RTT guide](https://blog.cloudflare.com/introducing-0-rtt/) for details.

## Building

```bash
cargo build --release
```

## License

MIT
