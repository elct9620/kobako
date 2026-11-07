//! The TLS the outbound seam ends on the host. It trusts the OS's own store,
//! so a revoked or added root follows the host's patches rather than a kobako
//! release.

use std::sync::{Arc, OnceLock};

use rustls::ClientConfig;
use rustls_platform_verifier::BuilderVerifierExt;

static PLATFORM: OnceLock<Option<Arc<ClientConfig>>> = OnceLock::new();

/// The client configuration trusting the OS's store, over ring, built once per
/// process; `None` where the platform offers no store to verify against. Its
/// first build reads the environment, `SSL_CERT_FILE` on Linux among it, so a
/// Wasm Driver builds it on the thread it is constructed on, holding its
/// frontend's lock, and a later change to that environment is not seen.
pub fn platform_tls() -> Option<Arc<ClientConfig>> {
    PLATFORM
        .get_or_init(|| {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .and_then(|builder| builder.with_platform_verifier())
                .map(|builder| Arc::new(builder.with_no_client_auth()))
                .ok()
        })
        .clone()
}
