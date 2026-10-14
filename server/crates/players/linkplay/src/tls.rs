//! TLS as LinkPlay devices need it: their own certificates are self-signed and not checked, and
//! those that ask for a client certificate are given LinkPlay's. That certificate is an old
//! X.509 version 1 one, which rustls will not parse when a client certificate is set the usual
//! way, so it is handed over by a resolver, which takes it as it is.

use std::sync::Arc;

use rustls::client::ResolvesClientCert;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::sign::CertifiedKey;
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use rustls_pki_types::pem::PemObject;

/// A client config that trusts any device and presents `certificate` (PEM: the certificate and
/// its key) to those that ask.
pub fn client_config(certificate: &[u8]) -> Result<ClientConfig, String> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(certificate)
        .collect::<Result<_, _>>()
        .map_err(|e| format!("cannot read the certificate: {e}"))?;
    if chain.is_empty() {
        return Err("no certificate in the file".to_owned());
    }
    let key = PrivateKeyDer::from_pem_slice(certificate)
        .map_err(|e| format!("cannot read the key: {e}"))?;
    let key = provider
        .key_provider
        .load_private_key(key)
        .map_err(|e| format!("cannot use the key: {e}"))?;
    let schemes = provider
        .signature_verification_algorithms
        .supported_schemes();
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyDevice { schemes }))
        .with_client_cert_resolver(Arc::new(Always(Arc::new(CertifiedKey::new(chain, key)))));
    Ok(config)
}

/// Accepts whatever certificate a device shows: they are self-signed, for no name.
#[derive(Debug)]
struct AnyDevice {
    schemes: Vec<SignatureScheme>,
}

impl ServerCertVerifier for AnyDevice {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes.clone()
    }
}

/// Presents the one certificate to every device that asks.
#[derive(Debug)]
struct Always(Arc<CertifiedKey>);

impl ResolvesClientCert for Always {
    fn resolve(
        &self,
        _root_hint_subjects: &[&[u8]],
        _sigschemes: &[SignatureScheme],
    ) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }

    fn has_certs(&self) -> bool {
        true
    }
}
