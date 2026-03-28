# Qrux

**Qrux** is a QUIC / HTTP/3–terminating proxy that forwards traffic to HTTP/1.1 backends over TCP.

- **Docs:** [dedsecrattle.github.io/Qrux](https://dedsecrattle.github.io/Qrux/)
- **Source:** [github.com/dedsecrattle/Qrux](https://github.com/dedsecrattle/Qrux)
- **Crates.io:** [crates.io/crates/qrux](https://crates.io/crates/qrux)

## Quick run

Map **UDP** for QUIC and mount your config + TLS certs:

```bash
docker run --rm \
  -p 8443:8443/udp \
  -p 8444:8444 \
  -p 9090:9090 \
  -v /path/to/proxy.toml:/etc/qrux/proxy.toml:ro \
  -v /path/to/certs:/etc/qrux/certs:ro \
  dedsecrattle/qrux:latest
```

See the repository **README** and **docker-compose** example for full options.
