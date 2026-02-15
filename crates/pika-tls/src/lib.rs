use std::sync::Once;

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

/// Build a TLS client config that works across iOS/Android/desktop without requiring
/// platform-specific initialization.
///
/// Strategy:
/// - Try OS/native roots via `rustls-native-certs`.
/// - If that yields 0 roots (common on iOS/Android), fall back to `webpki-roots`
///   (Mozilla bundle).
pub fn client_config() -> rustls::ClientConfig {
    init_rustls_crypto_provider();

    let mut roots = rustls::RootCertStore::empty();

    let native = rustls_native_certs::load_native_certs();
    for cert in native.certs {
        // Ignore individual bad certs; we only need a working store.
        let _ = roots.add(cert);
    }

    if roots.is_empty() {
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }

    rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth()
}
