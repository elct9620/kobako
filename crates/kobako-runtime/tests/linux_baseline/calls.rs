//! The calls a sequence is made of, drawn from a seed so a run repeats and an
//! older commit can be held to the same one.

/// How many descriptors a sequence holds at once.
pub const SLOTS: usize = 4;

/// The modified time a call sets, seconds past the Unix epoch.
pub const UTIME_SECONDS: i64 = 1;

/// One call, made alike through the VFS and through Linux. A path call starts
/// from the root, or from the descriptor `base` names.
#[derive(Clone, Debug)]
pub enum Call {
    Open(Open),
    Close(usize),
    Read(usize, usize),
    Write(usize, usize),
    Pread(usize, usize, u64),
    Pwrite(usize, usize, u64),
    Seek(usize, i64, u8),
    Truncate(usize, u64),
    Fallocate(usize, u64, u64),
    Fstat(usize),
    Fsync(usize),
    Readdir(usize),
    Stat {
        base: Option<usize>,
        path: String,
        follow: bool,
    },
    Utimes {
        base: Option<usize>,
        path: String,
        follow: bool,
    },
    Mkdir(Option<usize>, String),
    Rmdir(Option<usize>, String),
    Unlink(Option<usize>, String),
    Rename(Option<usize>, String, String),
}

/// What an open asks for, and the descriptor it lands in.
#[derive(Clone, Debug, Default)]
pub struct Open {
    pub base: Option<usize>,
    pub path: String,
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub exclusive: bool,
    pub truncate: bool,
    pub append: bool,
    pub directory: bool,
    pub nofollow: bool,
    pub slot: usize,
}

impl Call {
    /// The call's name, which a difference and a coverage count are kept by.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Open(_) => "Open",
            Self::Close(_) => "Close",
            Self::Read(..) => "Read",
            Self::Write(..) => "Write",
            Self::Pread(..) => "Pread",
            Self::Pwrite(..) => "Pwrite",
            Self::Seek(..) => "Seek",
            Self::Truncate(..) => "Truncate",
            Self::Fallocate(..) => "Fallocate",
            Self::Utimes { .. } => "Utimes",
            Self::Fstat(_) => "Fstat",
            Self::Fsync(_) => "Fsync",
            Self::Readdir(_) => "Readdir",
            Self::Stat { .. } => "Stat",
            Self::Mkdir(..) => "Mkdir",
            Self::Rmdir(..) => "Rmdir",
            Self::Unlink(..) => "Unlink",
            Self::Rename(..) => "Rename",
        }
    }
}

/// The bytes a write of `len` carries.
pub fn payload(len: usize) -> &'static [u8] {
    &b"WXYZ"[..len.min(4)]
}

/// A small generator, so a seed names the same sequence on every toolchain.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut rng = Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for _ in 0..4 {
            rng.next();
        }
        rng
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn random_below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn random_bool(&mut self, percent: u64) -> bool {
        self.random_below(100) < percent
    }

    fn choose_path(&mut self, paths: &[String]) -> String {
        paths[self.random_below(paths.len() as u64) as usize].clone()
    }

    /// The next call of a sequence over `paths`.
    pub fn call(&mut self, paths: &[String]) -> Call {
        let slot = self.random_below(SLOTS as u64) as usize;
        let base = self
            .random_bool(25)
            .then(|| self.random_below(SLOTS as u64) as usize);
        match self.random_below(100) {
            0..=27 => Call::Open(Open {
                base,
                path: self.choose_path(paths),
                read: self.random_bool(70),
                write: self.random_bool(45),
                create: self.random_bool(30),
                exclusive: self.random_bool(10),
                truncate: self.random_bool(10),
                append: self.random_bool(12),
                directory: self.random_bool(12),
                nofollow: self.random_bool(15),
                slot,
            }),
            28..=32 => Call::Close(slot),
            33..=38 => Call::Read(slot, self.random_below(8) as usize),
            39..=45 => Call::Write(slot, self.random_below(4) as usize),
            46..=48 => Call::Pread(slot, self.random_below(8) as usize, self.random_below(10)),
            49..=51 => Call::Pwrite(slot, self.random_below(4) as usize, self.random_below(10)),
            52..=54 => Call::Seek(
                slot,
                self.random_below(14) as i64 - 4,
                self.random_below(3) as u8,
            ),
            55..=58 => Call::Truncate(slot, self.random_below(12)),
            59..=62 => Call::Fstat(slot),
            63..=64 => Call::Fsync(slot),
            65..=68 => Call::Readdir(slot),
            69..=73 => Call::Stat {
                base,
                path: self.choose_path(paths),
                follow: self.random_bool(50),
            },
            74..=79 => Call::Mkdir(base, self.choose_path(paths)),
            80..=85 => Call::Rmdir(base, self.choose_path(paths)),
            86..=89 => Call::Unlink(base, self.choose_path(paths)),
            90 => Call::Fallocate(slot, self.random_below(10), self.random_below(5)),
            91 => Call::Utimes {
                base,
                path: self.choose_path(paths),
                follow: self.random_bool(50),
            },
            _ => Call::Rename(base, self.choose_path(paths), self.choose_path(paths)),
        }
    }
}
