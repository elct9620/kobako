//! The caps a Sandbox starts under when its Host App names none, as Linux's
//! `INIT_RLIMITS` sets what the first task starts with. Each cap is kept by
//! this crate, so every frontend and every Wasm Driver reads its default here.

use std::time::Duration;

/// The caps of one Sandbox. `None` on any field disables that cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    pub timeout: Option<Duration>,
    pub setup_timeout: Option<Duration>,
    pub memory_limit: Option<usize>,
    pub stdout_limit: Option<usize>,
    pub stderr_limit: Option<usize>,
    pub open_file_limit: Option<usize>,
    pub open_connection_limit: Option<usize>,
    pub connection_limit: Option<usize>,
}

impl Default for Limits {
    /// A 60-second deadline, 10 seconds to set the Guest Binary up, 4 MiB of
    /// memory growth, sized for a guest whose every value takes 16 bytes,
    /// 1 MiB for each output channel, 1024 open files, the soft limit Linux
    /// gives a process (`INR_OPEN_CUR`), and 6 open connections and 50 in
    /// all, as a Cloudflare Worker may hold 6 waiting and make 50
    /// subrequests on its free plan.
    fn default() -> Self {
        Limits {
            timeout: Some(Duration::from_secs(60)),
            setup_timeout: Some(Duration::from_secs(10)),
            memory_limit: Some(4 << 20),
            stdout_limit: Some(1 << 20),
            stderr_limit: Some(1 << 20),
            open_file_limit: Some(1024),
            open_connection_limit: Some(6),
            connection_limit: Some(50),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Limits;

    #[test]
    fn a_sandbox_starts_under_the_caps_every_frontend_promises() {
        assert_eq!(
            Limits::default(),
            Limits {
                timeout: Some(Duration::from_secs(60)),
                setup_timeout: Some(Duration::from_secs(10)),
                memory_limit: Some(4 * 1024 * 1024),
                stdout_limit: Some(1024 * 1024),
                stderr_limit: Some(1024 * 1024),
                open_file_limit: Some(1024),
                open_connection_limit: Some(6),
                connection_limit: Some(50),
            },
            "a Sandbox given no caps must start under a 60 s deadline, 10 s of setup, 4 MiB of \
             memory, 1 MiB per output channel, 1024 open files, and 6 open connections of 50"
        );
    }
}
