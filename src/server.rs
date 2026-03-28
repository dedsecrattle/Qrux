use crate::config::Config;
use crate::https_fallback;
use crate::metrics;
use crate::proxy;
use crate::router::Router;
use crate::upstream::UpstreamPool;
use anyhow::{Context, Result};
use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::sync::Arc;

pub async fn run(config: Config) -> Result<()> {
    let router = Arc::new(Router::new(&config.routes));
    let pool = Arc::new(UpstreamPool::new(16)); // Max 16 idle connections per upstream
    let quic_port = config.server.listen.port();

    // Start HTTPS fallback server if configured
    if let Some(https_addr) = config.server.https_listen {
        // Load certs again for HTTPS fallback
        let (https_certs, https_key) = load_certs_and_key(&config)?;
        let router_clone = Arc::clone(&router);
        let pool_clone = Arc::clone(&pool);

        tokio::spawn(async move {
            if let Err(e) = https_fallback::run(
                https_addr,
                quic_port,
                https_certs,
                https_key,
                router_clone,
                pool_clone,
            )
            .await
            {
                tracing::error!(error = %e, "HTTPS fallback server error");
            }
        });
    }

    let (certs, key) = load_certs_and_key(&config)?;
    let mut rustls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("Failed to build TLS config")?;

    // ALPN protocols for HTTP/3 - Chrome uses "h3"
    rustls_config.alpn_protocols = vec![b"h3".to_vec()];

    // Disable 0-RTT for now to simplify debugging
    // rustls_config.max_early_data_size = 0xFFFFFFFF;

    let quic_config = QuicServerConfig::try_from(rustls_config)?;
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_config));

    let endpoint = quinn::Endpoint::server(server_config, config.server.listen)
        .context("Failed to bind QUIC endpoint")?;

    tracing::info!(addr = %config.server.listen, "QUIC server listening (0-RTT enabled)");

    // Start metrics server if configured
    if let Some(metrics_addr) = config.server.metrics_listen {
        tokio::spawn(async move {
            if let Err(e) = metrics::serve_metrics(metrics_addr).await {
                tracing::error!(error = %e, "Metrics server error");
            }
        });
    }

    while let Some(incoming) = endpoint.accept().await {
        let router = Arc::clone(&router);
        let pool = Arc::clone(&pool);
        tokio::spawn(async move {
            metrics::inc_connections();
            let result = handle_connection(incoming, router, pool).await;
            metrics::dec_connections();
            if let Err(e) = result {
                tracing::error!(error = %e, "Connection error");
            }
        });
    }

    Ok(())
}

async fn handle_connection(
    incoming: quinn::Incoming,
    router: Arc<Router>,
    pool: Arc<UpstreamPool>,
) -> Result<()> {
    let connection = incoming.await.context("Failed to accept connection")?;

    let sni = connection
        .handshake_data()
        .and_then(|h| h.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
        .and_then(|h| h.server_name.clone());

    tracing::info!(
        remote = %connection.remote_address(),
        sni = ?sni,
        "New QUIC connection"
    );

    proxy::handle_connection(connection, router, pool, sni).await
}

fn load_certs_and_key(
    config: &Config,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert_path = &config.server.cert;
    let key_path = &config.server.key;

    let cert_file = std::fs::File::open(cert_path)
        .with_context(|| format!("Failed to open cert file: {}", cert_path.display()))?;
    let mut cert_reader = std::io::BufReader::new(cert_file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to parse certificates")?;

    if certs.is_empty() {
        anyhow::bail!("No certificates found in {}", cert_path.display());
    }

    let key_file = std::fs::File::open(key_path)
        .with_context(|| format!("Failed to open key file: {}", key_path.display()))?;
    let mut key_reader = std::io::BufReader::new(key_file);
    let key = rustls_pemfile::private_key(&mut key_reader)
        .context("Failed to read private key")?
        .ok_or_else(|| anyhow::anyhow!("No private key found in {}", key_path.display()))?;

    Ok((certs, key))
}
