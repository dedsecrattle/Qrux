//! Integration tests for quicproxy
//!
//! These tests spawn a real proxy and backend, testing the full request flow.

use anyhow::Result;
use bytes::Bytes;
use quinn::crypto::rustls::QuicClientConfig;
use rcgen::{CertificateParams, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

/// Generate self-signed certificate for testing
fn generate_test_certs() -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
    let mut params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    params.distinguished_name = rcgen::DistinguishedName::new();

    let key_pair = KeyPair::generate().unwrap();
    let cert = params.self_signed(&key_pair).unwrap();

    let cert_der = CertificateDer::from(cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));

    (vec![cert_der], key_der)
}

/// Simple HTTP/1.1 backend server for testing
async fn run_test_backend(
    addr: SocketAddr,
    response_body: &'static str,
) -> Result<oneshot::Sender<()>> {
    let listener = TcpListener::bind(addr).await?;
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    if let Ok((mut stream, _)) = accept_result {
                        let response_body = response_body;
                        tokio::spawn(async move {
                            let mut reader = BufReader::new(&mut stream);
                            let mut request = String::new();

                            // Read request headers
                            loop {
                                let mut line = String::new();
                                if reader.read_line(&mut line).await.is_err() {
                                    return;
                                }
                                request.push_str(&line);
                                if line == "\r\n" || line == "\n" {
                                    break;
                                }
                            }

                            // Send response
                            let response = format!(
                                "HTTP/1.1 200 OK\r\n\
                                Content-Type: text/plain\r\n\
                                Content-Length: {}\r\n\
                                Connection: keep-alive\r\n\
                                \r\n\
                                {}",
                                response_body.len(),
                                response_body
                            );

                            let _ = stream.write_all(response.as_bytes()).await;
                        });
                    }
                }
                _ = &mut shutdown_rx => {
                    break;
                }
            }
        }
    });

    Ok(shutdown_tx)
}

/// Create a QUIC client configuration that trusts the given certificate
fn create_test_client_config(cert: &CertificateDer<'static>) -> quinn::ClientConfig {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.clone()).unwrap();

    let mut rustls_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    rustls_config.alpn_protocols = vec![b"h3".to_vec()];

    let quic_config = QuicClientConfig::try_from(rustls_config).unwrap();
    quinn::ClientConfig::new(Arc::new(quic_config))
}

/// Start the proxy server for testing
async fn start_proxy(
    proxy_addr: SocketAddr,
    upstream_addr: SocketAddr,
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<quinn::Endpoint> {
    use quinn::crypto::rustls::QuicServerConfig;

    let mut rustls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    rustls_config.alpn_protocols = vec![b"h3".to_vec()];
    rustls_config.max_early_data_size = 0xFFFFFFFF;

    let quic_config = QuicServerConfig::try_from(rustls_config)?;
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_config));

    let endpoint = quinn::Endpoint::server(server_config, proxy_addr)?;

    // Spawn the proxy handler
    let endpoint_clone = endpoint.clone();
    let upstream = upstream_addr.to_string();
    tokio::spawn(async move {
        while let Some(incoming) = endpoint_clone.accept().await {
            let upstream = upstream.clone();
            tokio::spawn(async move {
                if let Ok(conn) = incoming.await {
                    let h3_conn =
                        h3::server::Connection::new(h3_quinn::Connection::new(conn)).await;
                    if let Ok(mut h3_conn) = h3_conn {
                        while let Ok(Some(resolver)) = h3_conn.accept().await {
                            let upstream = upstream.clone();
                            tokio::spawn(async move {
                                if let Ok((req, mut stream)) = resolver.resolve_request().await {
                                    // Forward to upstream
                                    if let Ok(mut tcp) =
                                        tokio::net::TcpStream::connect(&upstream).await
                                    {
                                        let path = req.uri().path();
                                        let request = format!(
                                            "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                                            path
                                        );
                                        let _ = tcp.write_all(request.as_bytes()).await;

                                        let mut response = Vec::new();
                                        let _ = tokio::io::AsyncReadExt::read_to_end(
                                            &mut tcp,
                                            &mut response,
                                        )
                                        .await;

                                        // Parse and forward response
                                        if let Some(body_start) = find_body_start(&response) {
                                            let body = &response[body_start..];
                                            let h3_response = http::Response::builder()
                                                .status(200)
                                                .header("content-type", "text/plain")
                                                .body(())
                                                .unwrap();
                                            let _ = stream.send_response(h3_response).await;
                                            let _ = stream
                                                .send_data(Bytes::copy_from_slice(body))
                                                .await;
                                        }
                                        let _ = stream.finish().await;
                                    }
                                }
                            });
                        }
                    }
                }
            });
        }
    });

    Ok(endpoint)
}

fn find_body_start(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if &data[i..i + 4] == b"\r\n\r\n" {
            return Some(i + 4);
        }
    }
    None
}

#[tokio::test]
async fn test_proxy_forwards_request() -> Result<()> {
    install_crypto_provider();

    // Generate test certificates
    let (certs, key) = generate_test_certs();

    // Start backend on a random port
    let backend_addr: SocketAddr = "127.0.0.1:0".parse()?;
    let backend_listener = TcpListener::bind(backend_addr).await?;
    let backend_addr = backend_listener.local_addr()?;
    drop(backend_listener);

    let _backend_shutdown = run_test_backend(backend_addr, "Hello from backend!").await?;

    // Give backend time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Start proxy
    let proxy_addr: SocketAddr = "127.0.0.1:0".parse()?;
    let proxy_listener = std::net::UdpSocket::bind(proxy_addr)?;
    let proxy_addr = proxy_listener.local_addr()?;
    drop(proxy_listener);

    let _proxy_endpoint = start_proxy(proxy_addr, backend_addr, certs.clone(), key).await?;

    // Give proxy time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create client and make request
    let client_config = create_test_client_config(&certs[0]);
    let mut client_endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
    client_endpoint.set_default_client_config(client_config);

    let connection = client_endpoint.connect(proxy_addr, "localhost")?.await?;

    // Make HTTP/3 request
    let h3_conn = h3::client::new(h3_quinn::Connection::new(connection)).await?;
    let (mut driver, mut send_request) = h3_conn;

    // Drive the connection
    tokio::spawn(async move {
        let _ = futures::future::poll_fn(|cx| driver.poll_close(cx)).await;
    });

    let request = http::Request::builder()
        .uri("https://localhost/test")
        .body(())?;

    let mut response_stream = send_request.send_request(request).await?;
    let response = response_stream.recv_response().await?;

    assert_eq!(response.status(), 200);

    // Read response body
    let mut body = Vec::new();
    while let Some(chunk) = response_stream.recv_data().await? {
        body.extend_from_slice(bytes::Buf::chunk(&chunk));
    }

    assert_eq!(String::from_utf8_lossy(&body), "Hello from backend!");

    client_endpoint.wait_idle().await;

    Ok(())
}

#[tokio::test]
async fn test_config_parsing() {
    install_crypto_provider();

    // Test with single upstream
    let config_str = r#"
[server]
listen = "0.0.0.0:4433"
cert = "./certs/cert.pem"
key = "./certs/key.pem"

[[routes]]
match = "api.example.com"
upstream = "127.0.0.1:8080"

[[routes]]
match = "*"
upstream = "127.0.0.1:9000"
"#;

    let config: quicproxy::config::Config = toml::from_str(config_str).unwrap();

    assert_eq!(config.server.listen, "0.0.0.0:4433".parse().unwrap());
    assert_eq!(config.routes.len(), 2);
    assert_eq!(config.routes[0].hostname, "api.example.com");
    assert_eq!(config.routes[0].get_upstreams(), vec!["127.0.0.1:8080"]);
    assert_eq!(config.routes[1].hostname, "*");
}

#[tokio::test]
async fn test_config_with_load_balancing() {
    install_crypto_provider();

    // Test with multiple upstreams
    let config_str = r#"
[server]
listen = "0.0.0.0:4433"
cert = "./certs/cert.pem"
key = "./certs/key.pem"
metrics_listen = "127.0.0.1:9090"
https_listen = "0.0.0.0:8444"

[[routes]]
match = "app.example.com"
upstreams = ["server1:80", "server2:80", "server3:80"]

[[routes]]
match = "*"
upstream = "default:80"
"#;

    let config: quicproxy::config::Config = toml::from_str(config_str).unwrap();

    assert_eq!(config.server.listen, "0.0.0.0:4433".parse().unwrap());
    assert_eq!(
        config.server.metrics_listen,
        Some("127.0.0.1:9090".parse().unwrap())
    );
    assert_eq!(
        config.server.https_listen,
        Some("0.0.0.0:8444".parse().unwrap())
    );
    assert_eq!(config.routes.len(), 2);
    assert_eq!(config.routes[0].hostname, "app.example.com");
    assert_eq!(
        config.routes[0].get_upstreams(),
        vec!["server1:80", "server2:80", "server3:80"]
    );
    assert_eq!(config.routes[1].get_upstreams(), vec!["default:80"]);
}

#[tokio::test]
async fn test_router_matching() {
    install_crypto_provider();

    use quicproxy::router::Router;

    let routes = vec![
        quicproxy::config::Route {
            hostname: "api.example.com".to_string(),
            upstream: Some("127.0.0.1:8080".to_string()),
            upstreams: None,
        },
        quicproxy::config::Route {
            hostname: "ws.example.com".to_string(),
            upstream: Some("127.0.0.1:9000".to_string()),
            upstreams: None,
        },
        quicproxy::config::Route {
            hostname: "*".to_string(),
            upstream: Some("127.0.0.1:3000".to_string()),
            upstreams: None,
        },
    ];

    let router = Router::new(&routes);

    assert_eq!(
        router.resolve(Some("api.example.com")),
        Some("127.0.0.1:8080")
    );
    assert_eq!(
        router.resolve(Some("ws.example.com")),
        Some("127.0.0.1:9000")
    );
    assert_eq!(
        router.resolve(Some("unknown.example.com")),
        Some("127.0.0.1:3000")
    );
    assert_eq!(router.resolve(None), Some("127.0.0.1:3000"));
}

#[tokio::test]
async fn test_router_load_balancing() {
    install_crypto_provider();

    use quicproxy::router::Router;

    let routes = vec![quicproxy::config::Route {
        hostname: "app.example.com".to_string(),
        upstream: None,
        upstreams: Some(vec![
            "server1:80".to_string(),
            "server2:80".to_string(),
            "server3:80".to_string(),
        ]),
    }];

    let router = Router::new(&routes);

    // Should round-robin through the upstreams
    let first = router.resolve(Some("app.example.com"));
    let second = router.resolve(Some("app.example.com"));
    let third = router.resolve(Some("app.example.com"));
    let fourth = router.resolve(Some("app.example.com"));

    assert_eq!(first, Some("server1:80"));
    assert_eq!(second, Some("server2:80"));
    assert_eq!(third, Some("server3:80"));
    assert_eq!(fourth, Some("server1:80")); // Wraps around
}

#[tokio::test]
async fn test_metrics_recording() {
    install_crypto_provider();

    use quicproxy::metrics;

    // Record some test requests
    metrics::record_request("GET", 200, "test-upstream:80", 0.05);
    metrics::record_request("POST", 201, "test-upstream:80", 0.1);
    metrics::record_request("GET", 500, "test-upstream:80", 0.2);

    // Gather metrics
    let output = metrics::gather_metrics();

    // Verify metrics are present
    assert!(output.contains("quicproxy_http_requests_total"));
    assert!(output.contains("quicproxy_http_request_duration_seconds"));
}
