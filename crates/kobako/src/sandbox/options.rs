use std::time::Duration;

use kobako_runtime::limits::Limits;
use kobako_runtime::profile::Profile;

/// Per-Sandbox caps and profile, the counterpart of the Ruby
/// `SandboxOptions` value object. `None` means "no cap".
///
/// Field-for-field the same shape as the wasmtime driver's `Config`, and
/// deliberately not that type: naming an engine's type here would make the
/// engine unremovable, and the SDK holds itself to an engine a host can
/// take out.
#[derive(Clone)]
#[non_exhaustive]
pub struct Options {
    /// Wall-clock cap for one invocation.
    pub timeout: Option<Duration>,
    /// Wall-clock cap for setting the Guest Binary up the first time its
    /// path is loaded; past it, construction fails.
    pub setup_timeout: Option<Duration>,
    /// Guest linear-memory cap, in bytes.
    pub memory_limit: Option<usize>,
    /// Captured-stdout cap, in bytes.
    pub stdout_limit: Option<usize>,
    /// Captured-stderr cap, in bytes.
    pub stderr_limit: Option<usize>,
    /// Descriptors one invocation may hold open, the standard ones and
    /// each Mount's included.
    pub open_file_limit: Option<usize>,
    /// Connections one Sandbox holds open at once, each socket, HTTP request,
    /// and name lookup in flight one.
    pub open_connection_limit: Option<usize>,
    /// Connections one invocation makes in all, each socket and HTTP request
    /// one.
    pub connection_limit: Option<usize>,
    /// Requested isolation floor; the driver declares its profile and
    /// construction fails below the floor.
    pub profile: Profile,
}

impl Default for Options {
    /// Secure by default: a 60-second deadline, 10 seconds to set the
    /// Guest Binary up, 4 MiB for memory, 1 MiB for each output channel,
    /// 1024 open files, 6 open connections and 50 in all, and the hermetic
    /// floor.
    fn default() -> Self {
        let limits = Limits::default();
        Options {
            timeout: limits.timeout,
            setup_timeout: limits.setup_timeout,
            memory_limit: limits.memory_limit,
            stdout_limit: limits.stdout_limit,
            stderr_limit: limits.stderr_limit,
            open_file_limit: limits.open_file_limit,
            open_connection_limit: limits.open_connection_limit,
            connection_limit: limits.connection_limit,
            profile: Profile::default(),
        }
    }
}
