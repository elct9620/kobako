//! Per-`Driver` execution configuration.
//!
//! Every cap a frontend forwards into `Driver::new`. A plain value carrier
//! owned by the `Driver` — distinct from the process-wide engine/module
//! `crate::cache` (which is shared across every sandbox) and from the
//! per-invocation `crate::invocation::Invocation` (which the wasm engine
//! mutates from inside a run).
//!
//! Field-for-field the same shape as the SDK's `kobako::Options`, and
//! deliberately not shared with it: this struct is free to name a wasmtime
//! type, and `Options` is not — an engine-free host must still be able to
//! build the SDK.

use std::time::Duration;

use kobako_runtime::profile::Profile;

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
    pub open_file_limit: Option<usize>,
    /// Isolation posture the frontend requested. `Hermetic` freezes
    /// ambient time and entropy, `Permissive` leaves the live WASI
    /// sources, and `Driver::profile` declares it back as the built
    /// posture.
    pub profile: Profile,
}

impl Default for Config {
    /// The caps every frontend defaults to: a 60-second deadline, 10
    /// seconds to set the Guest Binary up, 4 MiB for memory, 1 MiB for
    /// each output channel, 1024 open files, and the hermetic floor.
    fn default() -> Self {
        Config {
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
                config.profile
            ),
            (
                Some(Duration::from_secs(60)),
                Some(Duration::from_secs(10)),
                Some(4 * 1024 * 1024),
                Some(1024 * 1024),
                Some(1024 * 1024),
                Some(1024),
                Profile::Hermetic
            ),
            "Config::default must carry the caps every frontend defaults to"
        );
    }
}
