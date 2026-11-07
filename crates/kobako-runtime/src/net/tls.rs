//! The TLS the outbound seam ends on the host. It trusts the OS's own store,
//! so a revoked or added root follows the host's patches rather than a kobako
//! release.

use std::sync::{Arc, OnceLock};

use rustls::crypto::CryptoProvider;
use rustls::ClientConfig;
use rustls_platform_verifier::Verifier;

static PLATFORM: OnceLock<Option<(Arc<CryptoProvider>, Arc<Verifier>)>> = OnceLock::new();

/// A client configuration trusting the OS's store, over ring, with a session
/// store of its own, so no session one Invocation opened resumes in another;
/// `None` where the platform offers no store to verify against.
///
/// The verifier is built once per process. Its first build reads the
/// environment, `SSL_CERT_FILE` on Linux among it, so a Wasm Driver builds it
/// on the thread it is constructed on, holding its frontend's lock, and a
/// later change to that environment is not seen.
pub fn platform_tls() -> Option<Arc<ClientConfig>> {
    let (provider, verifier) = PLATFORM
        .get_or_init(|| {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let verifier = Verifier::new(provider.clone()).ok()?;
            Some((provider, Arc::new(verifier)))
        })
        .as_ref()?;
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .ok()?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();
    Some(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_configuration_holds_a_session_store_of_its_own() {
        let (Some(first), Some(second)) = (platform_tls(), platform_tls()) else {
            return;
        };
        assert!(
            !Arc::ptr_eq(&first, &second),
            "two outbound seams must each hold their own TLS configuration, so no session one \
             Invocation opened resumes in another"
        );
    }
}
