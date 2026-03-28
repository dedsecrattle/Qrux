use crate::config::{Config, Limits};
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
    let limits = Arc::new(config.server.limits.clone());
    let router = Arc::new(Router::new(&config.routes));
    let pool = Arc::new(UpstreamPool::new(
        config.server.limits.max_idle_connections_per_upstream,
    ));
    let quic_port = config.server.listen.port();

    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(16);

    if let Some(https_addr) = config.server.https_listen {
        let (https_certs, https_key) = load_certs_and_key(&config)?;
        let router_clone = Arc::clone(&router);
        let pool_clone = Arc::clone(&pool);
        let limits_clone = Arc::clone(&limits);
        let sub = shutdown_tx.subscribe();
        tokio::spawn(async move {
            if let Err(e) = https_fallback::run(
                https_addr,
                quic_port,
                https_certs,
                https_key,
                router_clone,
                pool_clone,
                limits_clone,
                sub,
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

    rustls_config.alpn_protocols = vec![b"h3".to_vec()];

    let quic_config = QuicServerConfig::try_from(rustls_config)?;
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_config));

    let endpoint = quinn::Endpoint::server(server_config, config.server.listen)
        .context("Failed to bind QUIC endpoint")?;

    tracing::info!(addr = %config.server.listen, "QUIC server listening");

    if let Some(metrics_addr) = config.server.metrics_listen {
        let sub = shutdown_tx.subscribe();
        tokio::spawn(async move {
            if let Err(e) = metrics::serve_metrics(metrics_addr, sub).await {
                tracing::error!(error = %e, "Metrics server error");
            }
        });
    }

    let mut shutdown_fut = Box::pin(shutdown_signal());

    loop {
        tokio::select! {
            _ = &mut shutdown_fut => {
                tracing::info!("Shutdown signal received, draining connections");
                let _ = shutdown_tx.send(());
                endpoint.close(0u32.into(), b"server shutdown");
                break;
            }
            incoming = endpoint.accept() => {
                match incoming {
                    None => {
                        tracing::info!("QUIC accept ended");
                        break;
                    }
                    Some(incoming) => {
                        let router = Arc::clone(&router);
                        let pool = Arc::clone(&pool);
                        let limits = Arc::clone(&limits);
                        tokio::spawn(async move {
                            metrics::inc_connections();
                            let result = accept_quic_connection(incoming, router, pool, limits).await;
                            metrics::dec_connections();
                            if let Err(e) = result {
                                tracing::error!(error = %e, "Connection error");
                            }
                        });
                    }
                }
            }
        }
    }

    tracing::info!(
        secs = limits.graceful_shutdown_secs,
        "Waiting for QUIC connections to finish"
    );
    match tokio::time::timeout(limits.graceful_shutdown(), endpoint.wait_idle()).await {
        Ok(()) => tracing::info!("Graceful shutdown complete"),
        Err(_) => tracing::warn!("Graceful shutdown timed out; exiting"),
    }

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut sig) = signal(SignalKind::terminate()) {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

async fn accept_quic_connection(
    incoming: quinn::Incoming,
    router: Arc<Router>,
    pool: Arc<UpstreamPool>,
    limits: Arc<Limits>,
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

    proxy::handle_connection(connection, router, pool, sni, limits).await
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
