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

impl Limits {
    /// Refuse a cap of zero, which would end every run at once rather than
    /// bound it, as `setrlimit` takes no limit below what a task needs to
    /// start; `None` is how a Host App asks for no cap.
    pub fn check(&self) -> Result<(), ZeroCap> {
        let durations = [
            ("timeout", self.timeout),
            ("setup_timeout", self.setup_timeout),
        ];
        let counts = [
            ("memory_limit", self.memory_limit),
            ("stdout_limit", self.stdout_limit),
            ("stderr_limit", self.stderr_limit),
            ("open_file_limit", self.open_file_limit),
            ("open_connection_limit", self.open_connection_limit),
            ("connection_limit", self.connection_limit),
        ];
        let zero_durations = durations
            .into_iter()
            .filter(|(_, cap)| *cap == Some(Duration::ZERO));
        let zero_counts = counts.into_iter().filter(|(_, cap)| *cap == Some(0));
        let zero = zero_durations
            .map(|(name, _)| name)
            .chain(zero_counts.map(|(name, _)| name))
            .next();
        match zero {
            Some(name) => Err(ZeroCap(name)),
            None => Ok(()),
        }
    }
}

/// A cap given as zero, named by its option.
#[derive(Debug, PartialEq, Eq)]
pub struct ZeroCap(&'static str);

impl std::fmt::Display for ZeroCap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} must be > 0, or none for no cap", self.0)
    }
}

impl std::error::Error for ZeroCap {}

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

    // @behavior RT-058
    #[test]
    fn a_cap_of_zero_is_refused() {
        let limits = Limits {
            connection_limit: Some(0),
            ..Limits::default()
        };
        let deadline = Limits {
            timeout: Some(Duration::ZERO),
            ..Limits::default()
        };
        assert_eq!(
            (
                limits.check().map_err(|e| e.to_string()),
                deadline.check().map_err(|e| e.to_string()),
                Limits::default().check().is_ok(),
            ),
            (
                Err("connection_limit must be > 0, or none for no cap".to_string()),
                Err("timeout must be > 0, or none for no cap".to_string()),
                true,
            ),
            "a connection_limit or timeout of zero through Limits::check must be refused by name, \
             and the default caps allowed"
        );
    }
}
