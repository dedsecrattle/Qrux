use anyhow::{Context, Result};
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub routes: Vec<Route>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    pub cert: PathBuf,
    pub key: PathBuf,
    /// Optional metrics endpoint (e.g., "127.0.0.1:9090")
    #[serde(default)]
    pub metrics_listen: Option<SocketAddr>,
    /// Optional HTTPS fallback listener for Alt-Svc discovery
    #[serde(default)]
    pub https_listen: Option<SocketAddr>,
    /// Timeouts, body limits, pool size, shutdown (all optional — see [`Limits::default`])
    #[serde(default)]
    pub limits: Limits,
}

/// Production-oriented limits with safe defaults.
#[derive(Debug, Clone, Deserialize)]
pub struct Limits {
    /// TCP connect timeout to upstream backends
    #[serde(default = "default_connect_timeout_secs")]
    pub upstream_connect_timeout_secs: u64,
    /// End-to-end timeout for a single upstream request (connect + request + response)
    #[serde(default = "default_request_timeout_secs")]
    pub upstream_request_timeout_secs: u64,
    /// Max HTTP/3 request body size from clients
    #[serde(default = "default_max_request_body_bytes")]
    pub max_request_body_bytes: usize,
    /// Max response body read from an upstream (protects memory)
    #[serde(default = "default_max_upstream_response_body_bytes")]
    pub max_upstream_response_body_bytes: usize,
    /// Idle TCP connections kept per upstream host:port
    #[serde(default = "default_max_idle_connections_per_upstream")]
    pub max_idle_connections_per_upstream: usize,
    /// Max time to wait for QUIC connections to finish after SIGTERM/SIGINT
    #[serde(default = "default_graceful_shutdown_secs")]
    pub graceful_shutdown_secs: u64,
}

fn default_connect_timeout_secs() -> u64 {
    10
}

fn default_request_timeout_secs() -> u64 {
    120
}

fn default_max_request_body_bytes() -> usize {
    10 * 1024 * 1024
}

fn default_max_upstream_response_body_bytes() -> usize {
    50 * 1024 * 1024
}

fn default_max_idle_connections_per_upstream() -> usize {
    16
}

fn default_graceful_shutdown_secs() -> u64 {
    30
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            upstream_connect_timeout_secs: default_connect_timeout_secs(),
            upstream_request_timeout_secs: default_request_timeout_secs(),
            max_request_body_bytes: default_max_request_body_bytes(),
            max_upstream_response_body_bytes: default_max_upstream_response_body_bytes(),
            max_idle_connections_per_upstream: default_max_idle_connections_per_upstream(),
            graceful_shutdown_secs: default_graceful_shutdown_secs(),
        }
    }
}

impl Limits {
    pub fn upstream_connect_timeout(&self) -> Duration {
        Duration::from_secs(self.upstream_connect_timeout_secs)
    }

    pub fn upstream_request_timeout(&self) -> Duration {
        Duration::from_secs(self.upstream_request_timeout_secs)
    }

    pub fn graceful_shutdown(&self) -> Duration {
        Duration::from_secs(self.graceful_shutdown_secs)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Route {
    /// Hostname to match (SNI or Host header)
    #[serde(rename = "match")]
    pub hostname: String,
    /// Single upstream address (for backwards compatibility)
    #[serde(default)]
    pub upstream: Option<String>,
    /// Multiple upstream addresses for load balancing
    #[serde(default)]
    pub upstreams: Option<Vec<String>>,
}

impl Route {
    /// Get all upstreams for this route
    pub fn get_upstreams(&self) -> Vec<String> {
        if let Some(ref upstreams) = self.upstreams {
            upstreams.clone()
        } else if let Some(ref upstream) = self.upstream {
            vec![upstream.clone()]
        } else {
            vec![]
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config =
            toml::from_str(&contents).with_context(|| "Failed to parse config file")?;
        config.validate()?;
        Ok(config)
    }

    /// Fail fast on unusable configuration (empty routes, bad limits).
    pub fn validate(&self) -> Result<()> {
        if self.routes.is_empty() {
            anyhow::bail!("At least one [[routes]] entry is required");
        }
        for (i, route) in self.routes.iter().enumerate() {
            if route.get_upstreams().is_empty() {
                anyhow::bail!(
                    "routes[{}] (match={:?}) must set `upstream` or non-empty `upstreams`",
                    i,
                    route.hostname
                );
            }
        }

        let l = &self.server.limits;
        if l.upstream_connect_timeout_secs == 0 {
            anyhow::bail!("limits.upstream_connect_timeout_secs must be > 0");
        }
        if l.upstream_request_timeout_secs == 0 {
            anyhow::bail!("limits.upstream_request_timeout_secs must be > 0");
        }
        if l.max_request_body_bytes == 0 {
            anyhow::bail!("limits.max_request_body_bytes must be > 0");
        }
        if l.max_upstream_response_body_bytes == 0 {
            anyhow::bail!("limits.max_upstream_response_body_bytes must be > 0");
        }
        if l.max_idle_connections_per_upstream == 0 {
            anyhow::bail!("limits.max_idle_connections_per_upstream must be > 0");
        }
        if l.graceful_shutdown_secs == 0 {
            anyhow::bail!("limits.graceful_shutdown_secs must be > 0");
        }
        if l.upstream_request_timeout_secs < l.upstream_connect_timeout_secs {
            anyhow::bail!(
                "limits.upstream_request_timeout_secs must be >= upstream_connect_timeout_secs"
            );
        }

        Ok(())
    }
}
