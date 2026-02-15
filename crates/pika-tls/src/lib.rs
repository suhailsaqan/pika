use std::sync::Once;

use rustls::pki_types::pem::PemObject;

static INIT: Once = Once::new();

/// rustls 0.23 selects a process-level CryptoProvider.
///
/// The repo frequently ends up with both `ring` and `aws-lc-rs` in the dependency
/// graph (e.g. nostr-sdk + QUIC). We choose one up front to avoid runtime errors.
pub fn init_rustls_crypto_provider() {
    INIT.call_once(|| {
        // Prefer ring for compatibility with nostr-sdk.
        // If another provider was already installed, ignore the error.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Build a TLS client config using `webpki-roots` (Mozilla CA bundle).
///
/// This is intentionally simple and deterministic across platforms:
/// - Full certificate validation + hostname/SNI (rustls defaults).
/// - Same trust roots on iOS/Android/desktop (no OS trust store integration).
///
/// If/when we want OS trust store behavior, switch this crate to
/// `rustls-platform-verifier` and keep call sites unchanged.
pub fn client_config() -> rustls::ClientConfig {
    init_rustls_crypto_provider();

    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

/// Build a TLS client config like [`client_config`] but with additional PEM-encoded
/// root certificates (e.g. private infra CA).
///
/// Fails if the PEM can't be parsed or certificates can't be added to the store.
pub fn client_config_with_extra_roots_pem(
    extra_roots_pem: &[u8],
) -> Result<rustls::ClientConfig, rustls::Error> {
    init_rustls_crypto_provider();

    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let mut any = false;
    for cert in rustls::pki_types::CertificateDer::pem_slice_iter(extra_roots_pem) {
        any = true;
        let cert = cert.map_err(|e| {
            rustls::Error::General(format!("failed to parse extra root cert PEM: {e}"))
        })?;
        roots.add(cert)?;
    }
    if !any {
        return Err(rustls::Error::General(
            "no CERTIFICATE sections found in extra root PEM".to_owned(),
        ));
    }

    Ok(rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}
