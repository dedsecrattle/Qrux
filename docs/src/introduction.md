# Introduction

**Qrux** is a QUIC / HTTP/3–terminating proxy that forwards traffic to plain TCP / HTTP/1.1 backends.

```text
Client (HTTP/3 over QUIC) ──→ [Qrux] ──→ Backend (HTTP/1.1 over TCP)
```

## Features

- **QUIC with TLS 1.3** (quinn + rustls)
- **HTTP/3** support
- **SNI / Host–based routing** to multiple backends
- **Round-robin load balancing** across multiple upstreams
- **Upstream TCP connection pooling**
- **Prometheus metrics** (`/metrics`)
- **HTTPS fallback** with `Alt-Svc` for browser HTTP/3 discovery
- **0-RTT** support for returning clients (understand the tradeoffs — see [Security](security.md))

## Links

- **Crates.io (releases, metadata):** [crates.io/crates/qrux](https://crates.io/crates/qrux)
- **Source code & issues:** [github.com/dedsecrattle/Qrux](https://github.com/dedsecrattle/Qrux)
- **This book (GitHub Pages):** [dedsecrattle.github.io/Qrux](https://dedsecrattle.github.io/Qrux/)

## License

MIT.
