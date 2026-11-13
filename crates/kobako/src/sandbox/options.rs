use kobako_runtime::limits::Limits;
use kobako_runtime::profile::Profile;

/// Per-Sandbox caps and profile, the counterpart of the Ruby
/// `SandboxOptions` value object.
///
/// It holds the runtime's own types, as the wasmtime driver's `Config` does,
/// and names no engine's: an engine a host can take out leaves the SDK
/// buildable.
#[derive(Clone)]
#[non_exhaustive]
pub struct Options {
    /// The caps every invocation runs under; `None` on any of them means no
    /// cap.
    pub limits: Limits,
    /// Requested isolation floor; the driver declares its profile and
    /// construction fails below the floor.
    pub profile: Profile,
}

impl Default for Options {
    /// Secure by default: the runtime's default caps and the hermetic floor.
    fn default() -> Self {
        Options {
            limits: Limits::default(),
            profile: Profile::default(),
        }
    }
}
