use std::time::Duration;

use kobako_runtime::profile::Profile;

/// Per-Sandbox caps and posture, the counterpart of the Ruby
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
    /// Requested isolation floor; the driver declares its posture and
    /// construction fails below the floor.
    pub profile: Profile,
}

impl Default for Options {
    /// Secure by default: a 60-second deadline, 10 seconds to set the
    /// Guest Binary up, 4 MiB for memory, 1 MiB for each output channel,
    /// 1024 open files, and the hermetic floor — the same defaults the
    /// Ruby frontend applies.
    fn default() -> Self {
        Options {
            timeout: Some(Duration::from_secs(60)),
            setup_timeout: Some(Duration::from_secs(10)),
            memory_limit: Some(4 << 20),
            stdout_limit: Some(1 << 20),
            stderr_limit: Some(1 << 20),
            open_file_limit: Some(1024),
            profile: Profile::Hermetic,
        }
    }
}
