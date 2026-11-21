//! The TLS the outbound seam ends on the host. It trusts the OS's own store,
//! so a revoked or added root follows the host's patches rather than a kobako
//! release.

use std::sync::{Arc, OnceLock};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, DistinguishedName, SignatureScheme};
use rustls_platform_verifier::Verifier;

static PLATFORM: OnceLock<Option<(Arc<CryptoProvider>, Arc<Verifier>)>> = OnceLock::new();

/// The server whose certificate the system first verified in this process,
/// which a forked child inherits as it inherits what verifying left behind.
static FIRST_TRUSTED: OnceLock<String> = OnceLock::new();

/// The server whose certificate macOS first verified in this process, `None`
/// before then and on every other system. From then on a forked child that
/// touches the system's certificate services crashes, as the threads serving
/// them did not come along; Linux reads its certificates from files.
pub fn first_trusted_host() -> Option<&'static str> {
    FIRST_TRUSTED.get().map(String::as_str)
}

/// The platform's verifier, recording the first server it had the system
/// verify.
#[derive(Debug)]
struct FirstTrustRecording(Arc<Verifier>);

impl ServerCertVerifier for FirstTrustRecording {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if cfg!(target_os = "macos") {
            FIRST_TRUSTED.get_or_init(|| server_name.to_str().into_owned());
        }
        self.0
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }

    fn root_hint_subjects(&self) -> Option<&[DistinguishedName]> {
        self.0.root_hint_subjects()
    }
}

/// The OS's store and the provider verifying against it, built once per
/// process, `None` where the platform offers no store. Its first build reads
/// the environment, `SSL_CERT_FILE` on Linux among it, so it is built as the
/// first network entry is made, on the Host App's thread building its
/// Sandbox, rather than within a handshake beside another reading it; a later
/// change to that environment is not seen.
pub(crate) fn load_platform_verifier() -> Option<&'static (Arc<CryptoProvider>, Arc<Verifier>)> {
    PLATFORM
        .get_or_init(|| {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let verifier = Verifier::new(provider.clone()).ok()?;
            Some((provider, Arc::new(verifier)))
        })
        .as_ref()
}

/// A client configuration trusting the OS's store, over ring, with a session
/// store of its own, so no session one Invocation opened resumes in another;
/// `None` where the platform offers no store to verify against.
pub fn new_platform_tls() -> Option<Arc<ClientConfig>> {
    let (provider, verifier) = load_platform_verifier()?;
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .ok()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(FirstTrustRecording(verifier.clone())))
        .with_no_client_auth();
    Some(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_configuration_holds_a_session_store_of_its_own() {
        let (Some(first), Some(second)) = (new_platform_tls(), new_platform_tls()) else {
            return;
        };
        assert!(
            !Arc::ptr_eq(&first, &second),
            "two outbound seams must each hold their own TLS configuration, so no session one \
             Invocation opened resumes in another"
        );
    }
}
