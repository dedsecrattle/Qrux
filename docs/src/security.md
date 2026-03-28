# Security

## 0-RTT (early data)

0-RTT lets returning clients send application data in the first flight, which improves latency but **data can be replayed** by an attacker who captures the encrypted packets.

**Generally reasonable:** idempotent reads (e.g. `GET`) where replay is acceptable.

**Avoid:** anything that must run exactly once — payments, `POST` / `PUT` / `DELETE`, or sensitive state changes without replay protection at the application layer.

Further reading: [Cloudflare — Introducing 0-RTT](https://blog.cloudflare.com/introducing-0-rtt/).

## TLS and certificates

- Use real certificates from a public CA in production.
- Protect private keys (`key` in config); never commit them to git.

## Upstream trust

The proxy terminates TLS from clients and opens new TCP connections to backends. Ensure network access to upstreams is restricted (firewall / VPC) to what the proxy actually needs.
