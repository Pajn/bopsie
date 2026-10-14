//! Finding a device's HTTP API: over plain HTTP, over HTTPS, and over HTTPS that demands a
//! client certificate, as Audio Pro's does.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bopsie_players_linkplay::transport::{Api, ApiError, CLIENT_CERTIFICATE};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const STATUS: &str = r#"{"DeviceName":"Kitchen","project":"Fake","group":"0","uuid":"0123456789ABCDEF01234567","MAC":"AA:BB:CC:00:11:22","wmrm_version":"4.2","streams":"0x1"}"#;

/// Requires a client certificate, and accepts any: as the devices do, as far as is known.
#[derive(Debug)]
struct RequireAny {
    schemes: Vec<SignatureScheme>,
}

impl ClientCertVerifier for RequireAny {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes.clone()
    }
    fn client_auth_mandatory(&self) -> bool {
        true
    }
}

/// Answers one HTTP request on `stream` with `body`.
async fn answer<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    mut stream: S,
    body: &str,
) {
    let mut request = [0u8; 4096];
    let _ = stream.read(&mut request).await;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// A device that answers only over TLS, and only to clients with a certificate. Returns its
/// port and whether a client was let in.
async fn mtls_device() -> (u16, Arc<AtomicBool>) {
    let generated = rcgen::generate_simple_self_signed(vec!["device.local".into()]).unwrap();
    let cert = CertificateDer::from(generated.cert.der().to_vec());
    let key = PrivateKeyDer::try_from(generated.signing_key.serialize_der()).unwrap();
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let schemes = provider
        .signature_verification_algorithms
        .supported_schemes();
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_client_cert_verifier(Arc::new(RequireAny { schemes }))
        .with_single_cert(vec![cert], key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let admitted = Arc::new(AtomicBool::new(false));
    let seen = admitted.clone();
    tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            let (acceptor, seen) = (acceptor.clone(), seen.clone());
            tokio::spawn(async move {
                if let Ok(tls) = acceptor.accept(tcp).await {
                    seen.store(true, Ordering::SeqCst);
                    answer(tls, STATUS).await;
                }
            });
        }
    });
    (port, admitted)
}

/// A device that answers plain HTTP.
async fn http_device() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            tokio::spawn(answer(tcp, STATUS));
        }
    });
    port
}

/// A port nothing listens on.
async fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    drop(listener);
    addr.port()
}

fn localhost() -> std::net::IpAddr {
    "127.0.0.1".parse().unwrap()
}

#[tokio::test]
async fn a_device_that_demands_a_client_certificate_gets_linkplays() {
    let closed = closed_port().await;
    let (port, admitted) = mtls_device().await;
    let api = Api::with_probes(
        localhost(),
        CLIENT_CERTIFICATE,
        &[("https", closed), ("https", port)],
    )
    .unwrap();
    let base = api.probe().await.unwrap();
    assert_eq!(base, format!("https://127.0.0.1:{port}"));
    assert!(admitted.load(Ordering::SeqCst));
    let status = api.command("getStatusEx").await.unwrap();
    assert!(status.contains("Kitchen"));
}

#[tokio::test]
async fn plain_http_is_tried_when_https_is_not_there() {
    let closed = closed_port().await;
    let port = http_device().await;
    let api = Api::with_probes(
        localhost(),
        CLIENT_CERTIFICATE,
        &[("https", closed), ("http", port)],
    )
    .unwrap();
    assert_eq!(
        api.probe().await.unwrap(),
        format!("http://127.0.0.1:{port}")
    );
}

#[tokio::test]
async fn nothing_answering_is_unreachable() {
    let closed = closed_port().await;
    let api = Api::with_probes(localhost(), CLIENT_CERTIFICATE, &[("https", closed)]).unwrap();
    assert_eq!(api.probe().await, Err(ApiError::Unreachable));
}

#[test]
fn a_file_that_is_not_a_certificate_is_refused() {
    assert!(matches!(
        Api::new(localhost(), b"not a certificate"),
        Err(ApiError::Certificate(_))
    ));
}
