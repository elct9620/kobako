//! The caps a `Driver` applies to every invocation.
//!
//! What a frontend forwards into `Driver::new`. A plain value carrier
//! owned by the `Driver` — distinct from the process-wide engine/module
//! `crate::cache` (which is shared across every sandbox) and from the
//! per-invocation `crate::invocation::Invocation` (which the wasm engine
//! mutates from inside a run).
//!
//! The caps and the profile are the runtime's own types, which the SDK's
//! `kobako::Options` holds as well; this struct alone may name a wasmtime
//! type, as an engine-free host must still be able to build the SDK.
//! The security modules are the Driver's own, as Linux's LSMs are the
//! kernel's: whoever builds the Driver hands them here.

use std::sync::Arc;

use kobako_runtime::limits::Limits;
use kobako_runtime::profile::Profile;
use kobako_runtime::security::SecurityOperations;

/// The caps and requested isolation profile for one `Driver`.
#[non_exhaustive]
pub struct Config {
    /// The caps every invocation runs under; `None` on any of them disables
    /// that cap.
    pub limits: Limits,
    /// Isolation profile the frontend requested. `Hermetic` freezes
    /// ambient time and entropy, `Permissive` gives the live host clocks
    /// and entropy, and `Driver::profile` declares it back as the built
    /// profile.
    pub profile: Profile,
    /// Security modules every invocation is judged by, each asked in order
    /// after kobako's own rules and before any Mount's Policy, as Linux
    /// stacks the LSMs it was booted with.
    pub security: Vec<Arc<dyn SecurityOperations>>,
}

impl Default for Config {
    /// A Sandbox's default caps and floor, and no security module beyond
    /// kobako's own.
    fn default() -> Self {
        Config {
            limits: Limits::default(),
            profile: Profile::default(),
            security: Vec::new(),
        }
    }
}

#[cfg(test)]
impl Config {
    /// A config at `profile` that sets no cap, so a test names only the cap it
    /// witnesses.
    pub(crate) fn for_profile(profile: Profile) -> Self {
        let mut limits = Limits::default();
        limits.timeout = None;
        limits.setup_timeout = None;
        limits.memory_limit = None;
        limits.stdout_limit = None;
        limits.stderr_limit = None;
        limits.open_file_limit = None;
        limits.open_connection_limit = None;
        limits.connection_limit = None;
        Config {
            limits,
            profile,
            security: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_config_carries_every_frontends_default_caps() {
        let config = Config::default();
        assert_eq!(
            (config.limits, config.profile, config.security.len()),
            (Limits::default(), Profile::Hermetic, 0),
            "Config::default must carry the runtime's default caps and the hermetic floor, and \
             no security module beyond kobako's own"
        );
    }
}
