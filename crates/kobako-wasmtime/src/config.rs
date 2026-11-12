//! The caps a `Driver` applies to every invocation.
//!
//! Every cap a frontend forwards into `Driver::new`. A plain value carrier
//! owned by the `Driver` — distinct from the process-wide engine/module
//! `crate::cache` (which is shared across every sandbox) and from the
//! per-invocation `crate::invocation::Invocation` (which the wasm engine
//! mutates from inside a run).
//!
//! Its caps are field-for-field the SDK's `kobako::Options`, and deliberately
//! not shared with it: this struct is free to name a wasmtime type, and
//! `Options` is not — an engine-free host must still be able to build the SDK.
//! The security modules are the Driver's own, as Linux's LSMs are the
//! kernel's: whoever builds the Driver hands them here.

use std::sync::Arc;
use std::time::Duration;

use kobako_runtime::profile::Profile;
use kobako_runtime::security::SecurityOperations;

/// The caps and requested isolation profile for one `Driver`. `None` on
/// any cap field disables that cap.
#[non_exhaustive]
pub struct Config {
    /// Wall-clock cap for one guest `#eval` / `#run`; the invocation traps
    /// once it passes.
    pub timeout: Option<Duration>,
    /// Wall-clock cap for setting the Guest Binary up the first time its
    /// path is loaded; past it, construction fails.
    pub setup_timeout: Option<Duration>,
    /// Guest linear-memory cap, in bytes, charged to what one invocation
    /// grows beyond the image it starts from.
    pub memory_limit: Option<usize>,
    /// Byte cap for guest stdout capture; output past it is dropped and
    /// the capture marked truncated.
    pub stdout_limit: Option<usize>,
    /// Byte cap for guest stderr capture, the same as `stdout_limit`.
    pub stderr_limit: Option<usize>,
    /// Descriptors one invocation may hold open, the standard ones and
    /// each Mount's preopen included; an open past it answers `MFILE`.
    /// Only the guest's descriptors count, as `RLIMIT_NOFILE` counts a
    /// process's: the host descriptors a Filesystem holds behind one are
    /// its own, up to two for a file and one for each Layer holding a
    /// directory, Directory Layers alone holding any.
    pub open_file_limit: Option<usize>,
    /// Connections one Sandbox holds open at once, each socket, HTTP request,
    /// and name lookup in flight one; one past it answers `EAGAIN`.
    pub open_connection_limit: Option<usize>,
    /// Connections one invocation makes in all, each socket and HTTP request
    /// one, as a budget of subrequests is counted; one past it answers
    /// `EAGAIN`.
    pub connection_limit: Option<usize>,
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
    /// The caps every frontend defaults to: a 60-second deadline, 10
    /// seconds to set the Guest Binary up, 4 MiB for memory, 1 MiB for
    /// each output channel, 1024 open files, 6 open connections and 50 in
    /// all, and the hermetic floor.
    fn default() -> Self {
        Config {
            timeout: Some(Duration::from_secs(60)),
            setup_timeout: Some(Duration::from_secs(10)),
            memory_limit: Some(4 << 20),
            stdout_limit: Some(1 << 20),
            stderr_limit: Some(1 << 20),
            open_file_limit: Some(1024),
            open_connection_limit: Some(6),
            connection_limit: Some(50),
            profile: Profile::Hermetic,
            security: Vec::new(),
        }
    }
}

#[cfg(test)]
impl Config {
    /// A config at `profile` that sets no cap, so a test names only the cap it
    /// witnesses.
    pub(crate) fn for_profile(profile: Profile) -> Self {
        Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            open_connection_limit: None,
            connection_limit: None,
            profile,
            security: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Literal values, so a drift from the defaults the SDK's Options and
    // the Ruby frontend apply fails here.
    #[test]
    fn a_default_config_carries_every_frontends_default_caps() {
        let config = Config::default();
        assert_eq!(
            (
                config.timeout,
                config.setup_timeout,
                config.memory_limit,
                config.stdout_limit,
                config.stderr_limit,
                config.open_file_limit,
                config.open_connection_limit,
                config.connection_limit,
                config.profile,
                config.security.len()
            ),
            (
                Some(Duration::from_secs(60)),
                Some(Duration::from_secs(10)),
                Some(4 * 1024 * 1024),
                Some(1024 * 1024),
                Some(1024 * 1024),
                Some(1024),
                Some(6),
                Some(50),
                Profile::Hermetic,
                0
            ),
            "Config::default must carry the caps every frontend defaults to, and no security \
             module beyond kobako's own"
        );
    }
}
