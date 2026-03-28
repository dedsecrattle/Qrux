use anyhow::{Context, Result};
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub routes: Vec<Route>,
}

#[derive(Debug, Deserialize)]
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

        toml::from_str(&contents).with_context(|| "Failed to parse config file")
    }
}
