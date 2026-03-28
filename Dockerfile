# Build: rustls/aws-lc-rs needs CMake to compile aws-lc-sys
FROM rust:1-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY examples ./examples

ENV RUSTFLAGS="-C target-cpu=generic"
RUN cargo build --locked --release

# Runtime: minimal glibc image (matches builder)
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/qrux /usr/local/bin/qrux

# QUIC is UDP; HTTPS fallback is TCP; metrics is TCP
EXPOSE 8443/udp 8444/tcp 9090/tcp

ENTRYPOINT ["/usr/local/bin/qrux"]
CMD ["--config", "/etc/qrux/proxy.toml"]
