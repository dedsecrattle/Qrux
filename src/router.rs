use crate::config::Route;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct UpstreamGroup {
    upstreams: Vec<String>,
    counter: AtomicUsize,
}

impl UpstreamGroup {
    fn new(upstreams: Vec<String>) -> Self {
        UpstreamGroup {
            upstreams,
            counter: AtomicUsize::new(0),
        }
    }

    fn next(&self) -> Option<&str> {
        if self.upstreams.is_empty() {
            return None;
        }
        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.upstreams.len();
        Some(&self.upstreams[idx])
    }

    fn len(&self) -> usize {
        self.upstreams.len()
    }
}

#[derive(Debug)]
pub struct Router {
    routes: HashMap<String, UpstreamGroup>,
    default_upstream: Option<UpstreamGroup>,
}

impl Router {
    pub fn new(routes: &[Route]) -> Self {
        let mut map = HashMap::new();
        let mut default = None;

        for route in routes {
            let upstreams = route.get_upstreams();
            if upstreams.is_empty() {
                continue;
            }

            let group = UpstreamGroup::new(upstreams);

            if route.hostname == "*" {
                default = Some(group);
            } else {
                map.insert(route.hostname.clone(), group);
            }
        }

        Router {
            routes: map,
            default_upstream: default,
        }
    }

    /// Resolve upstream address for a given hostname (from SNI or Host header)
    /// Uses round-robin load balancing when multiple upstreams are configured
    pub fn resolve(&self, hostname: Option<&str>) -> Option<&str> {
        if let Some(host) = hostname {
            // Strip port if present (Host header may include it)
            let host = host.split(':').next().unwrap_or(host);

            if let Some(group) = self.routes.get(host) {
                return group.next();
            }
        }

        self.default_upstream.as_ref().and_then(|g| g.next())
    }

    /// Get the number of upstreams for a hostname
    pub fn upstream_count(&self, hostname: Option<&str>) -> usize {
        if let Some(host) = hostname {
            let host = host.split(':').next().unwrap_or(host);
            if let Some(group) = self.routes.get(host) {
                return group.len();
            }
        }
        self.default_upstream.as_ref().map(|g| g.len()).unwrap_or(0)
    }
}
