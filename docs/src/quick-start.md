# Quick start

## 1. TLS certificates

For local development, [mkcert](https://github.com/FiloSottile/mkcert) is convenient:

```bash
brew install mkcert   # or your OS package manager
mkcert -install
cd certs && mkcert -key-file key.pem -cert-file cert.pem localhost 127.0.0.1 ::1
```

Alternatively, use the script in the repo:

```bash
./scripts/gen-certs.sh
```

## 2. Configuration

Create `proxy.toml`:

```toml
[server]
listen = "0.0.0.0:8443"           # QUIC/HTTP3 port
cert = "./certs/cert.pem"
key = "./certs/key.pem"
metrics_listen = "127.0.0.1:9090" # Prometheus metrics
https_listen = "0.0.0.0:8444"       # HTTPS fallback (Alt-Svc)

[[routes]]
match = "*"
upstream = "example.com:80"

# Or load balancing:
# upstreams = ["server1:80", "server2:80", "server3:80"]
```

## 3. Run the proxy

```bash
cargo run --release -- --config proxy.toml
```

## 4. Smoke test

```bash
# HTTP/3 (needs curl with HTTP/3)
curl --http3-only -k https://localhost:8443/

# Or HTTPS fallback (any curl)
curl -k https://localhost:8444/
```

## Build from source

```bash
cargo build --release
```

The binary is at `target/release/quicproxy`.
