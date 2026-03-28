# Architecture

High-level view of how listeners and routing fit together:

```text
┌─────────────────────────────────────────────────────┐
│                       Qrux                          │
│                                                     │
│  ┌──────────────┐  ┌──────────────┐                 │
│  │ QUIC/HTTP3   │  │ HTTPS        │  ◄── Alt-Svc   │
│  │ (listen)     │  │ (https_listen)│     header    │
│  └──────┬───────┘  └──────┬───────┘                 │
│         │                 │                         │
│         └────────┬────────┘                         │
│                  ▼                                  │
│         ┌────────────────┐                          │
│         │     Router     │  ◄── SNI / Host         │
│         │  (round-robin) │                          │
│         └────────┬───────┘                          │
│                  ▼                                  │
│         ┌────────────────┐                          │
│         │  Upstream pool │  ◄── TCP reuse          │
│         └────────┬───────┘                          │
│                  ▼                                  │
│         ┌────────────────┐                          │
│         │    Backends    │                          │
│         └────────────────┘                          │
└─────────────────────────────────────────────────────┘
```

## HTTPS fallback and `Alt-Svc`

The optional HTTPS listener responds over TLS (HTTP/1.1) and sends an **`Alt-Svc`** header pointing at the QUIC port so clients that speak HTTP/3 can discover and upgrade (browser behavior varies; `curl` is reliable for verifying HTTP/3).

Example header shape:

```http
Alt-Svc: h3=":8443"; ma=86400, h3-29=":8443"; ma=86400
```

Adjust ports to match your `listen` / `https_listen` configuration.
