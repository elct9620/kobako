//! Compares the VFS and the built-in Filesystems with a real Linux: one seeded
//! sequence of calls goes through kobako and through a real tmpfs, overlayfs, or
//! directory, and each answer and the tree left behind are compared. This is
//! evidence for a change: the VFS's baseline is Linux VFS's shape, and a
//! Filesystem's is the specification.
//!
//! `linux-baseline-expected.txt` names the differences known so far, which a line
//! there does not settle. A new one fails, and so does one no sequence reaches any
//! more. `concurrent` holds calls
//! racing on one instance, and `permissions` what a Policy and another user are
//! refused. `rake crates:linux_baseline` runs this as root in a container, since
//! overlayfs needs a mount.

#![cfg(target_os = "linux")]

mod calls;
mod concurrent;
mod kobako;
mod linux;
mod permissions;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::sleep;
use std::time::Duration;

use kobako_runtime::filesystem::{Bind, Filesystem, Overlay, Tmpfs};
use kobako_runtime::mount::Access;

use calls::{Call, Rng};
use kobako::Kobako;
use linux::{mount, on_tmpfs, snapshot_into, umount, Linux};

/// Every entry beneath a root, and what it shows.
pub type Snapshot = BTreeMap<String, String>;

/// When each entry beneath a root was last modified and changed.
pub type Times<T> = BTreeMap<String, (T, T)>;

/// Where each setup keeps its directories, a tmpfs in every container.
const BASE: &str = "/dev/shm/kobako-linux-baseline";

const EXPECTED: &str = include_str!("linux-baseline-expected.txt");

/// What an Overlay leaves out, turned off whatever this kernel defaults to, so
/// its answers are the ones kobako aligns with.
const OVERLAY_FEATURES_OFF: &str = "redirect_dir=off,metacopy=off,index=off,xino=off";

/// One setup runs at a time, as two sharing its directories would collide.
static SERIAL: Mutex<()> = Mutex::new(());

/// Longer than Linux's coarse clock ticks, so a change moves a time on both.
const CLOCK_TICK: Duration = Duration::from_millis(12);

/// What a stat shows, the same on both sides. A directory merged from Layers
/// reports no size both sides could agree on, and a link's size is where it
/// points, which a Filesystem may report its own way.
pub fn describe(kind: &str, len: u64, nlink: u64, merged: bool) -> String {
    match (kind, merged) {
        ("dir", true) => format!("dir nlink={nlink}"),
        ("lnk", _) => "lnk".to_string(),
        _ => format!("{kind} len={len} nlink={nlink}"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Setup {
    Tmpfs,
    Overlay,
    OverlayNoUpper,
    Bind,
    ReadOnlyBind,
}

impl Setup {
    fn name(self) -> &'static str {
        match self {
            Self::Tmpfs => "tmpfs",
            Self::Overlay => "overlay",
            Self::OverlayNoUpper => "overlay-no-upper",
            Self::Bind => "bind",
            Self::ReadOnlyBind => "read-only-bind",
        }
    }

    fn dir(self, name: &str) -> String {
        format!("{BASE}/{}/{name}", self.name())
    }

    fn merged(self) -> bool {
        matches!(self, Self::Overlay | Self::OverlayNoUpper)
    }

    fn changes(self) -> bool {
        matches!(self, Self::Tmpfs | Self::Overlay | Self::Bind)
    }

    /// What a sequence walks. A Tmpfs holds no links.
    fn paths(self) -> Vec<String> {
        let mut paths: Vec<String> = [
            "", "a", "b", "c", "d", "e", "m", "d/f", "d/g", "d/e", "d/e/h", "m/s", "m/x", "a/",
            "b/", "d/", "d/.", "d/..", "d/../a", ".", "a/b", "b/c", "d/e/..",
        ]
        .map(str::to_string)
        .to_vec();
        paths.push("x/".repeat(2048));
        paths.push("N".repeat(256));
        paths.push(format!("d/{}", "N".repeat(256)));
        paths.push("M".repeat(255));
        paths.extend(
            [
                "d/f/", "d/f/.", "d/f/..", "a/..", "./a", "d//f", "d/./f", "./", "d/e/../f", "d/e/",
            ]
            .map(str::to_string),
        );
        if self != Self::Tmpfs {
            paths.extend(["h1", "h2", "l", "l/", "n", "n/f", "n/", "g", "g/"].map(str::to_string));
            paths.extend(["l/x", "n/..", "n/../a", "n/e", "g/x", "n/e/.."].map(str::to_string));
        }
        paths
    }

    /// Lay out what a run starts from. What only one side keeps beneath the
    /// other's view is laid out once, as nothing a run does reaches a lower Layer.
    fn prepare(self) {
        let _ = fs::remove_dir_all(format!("{BASE}/{}", self.name()));
        fs::create_dir_all(format!("{BASE}/{}", self.name())).expect("the setup's directory");
        if self.merged() {
            seed_directory(&self.dir("kobako-lower"));
            seed_directory(&self.dir("linux-lower"));
        }
        if self == Self::OverlayNoUpper {
            for side in ["kobako-second", "linux-second"] {
                let dir = self.dir(side);
                fs::create_dir_all(format!("{dir}/m/y")).expect("the Layer's directory");
                fs::create_dir_all(format!("{dir}/z")).expect("the Layer's directory");
                fs::write(format!("{dir}/m/x"), "hidden").expect("the Layer's file");
                fs::write(format!("{dir}/b"), "low").expect("the Layer's file");
            }
        }
    }

    fn start(self, settle: bool) -> (Kobako, Linux) {
        match self {
            Self::Tmpfs => {
                let filesystem =
                    Filesystem::new(Arc::new(Tmpfs::new())).expect("the instance must stand");
                let mut kobako = Kobako::new(filesystem, Access::ReadWrite, false);
                let root = self.dir("linux");
                let _ = fs::remove_dir_all(&root);
                fs::create_dir_all(&root).expect("the tmpfs directory");
                let mut linux = Linux::new(&root, false);
                for call in tmpfs_seed() {
                    if settle {
                        sleep(CLOCK_TICK);
                    }
                    kobako.run(&call);
                    linux.run(&call);
                }
                (kobako, linux)
            }
            Self::Bind | Self::ReadOnlyBind => {
                let (kobako_dir, linux_dir) = (self.dir("kobako"), self.dir("linux"));
                let linux_root = self.dir("linux-mount");
                if self == Self::ReadOnlyBind {
                    umount(&linux_root);
                }
                seed_directory(&kobako_dir);
                seed_directory(&linux_dir);
                let filesystem =
                    Filesystem::new(Arc::new(bind(&kobako_dir))).expect("the instance must stand");
                let access = match self {
                    Self::ReadOnlyBind => Access::Read,
                    _ => Access::ReadWrite,
                };
                let kobako = Kobako::new(filesystem, access, false);
                if self == Self::Bind {
                    return (kobako, Linux::new(&linux_dir, false));
                }
                fs::create_dir_all(&linux_root).expect("the mount point");
                mount(&linux_dir, &linux_root, None, libc::MS_BIND, None);
                mount(
                    &linux_dir,
                    &linux_root,
                    None,
                    libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY,
                    None,
                );
                (kobako, Linux::new(&linux_root, false))
            }
            Self::Overlay | Self::OverlayNoUpper => self.start_overlay(),
        }
    }

    fn start_overlay(self) -> (Kobako, Linux) {
        let root = self.dir("linux-merged");
        umount(&root);
        fs::create_dir_all(&root).expect("the mount point");
        let lower = Filesystem::new(Arc::new(bind(&self.dir("kobako-lower"))))
            .expect("the instance must stand");
        let (overlay, data) = if self == Self::Overlay {
            let (upper, work) = (self.dir("linux-upper"), self.dir("linux-work"));
            for dir in [&upper, &work] {
                let _ = fs::remove_dir_all(dir);
                fs::create_dir_all(dir).expect("the overlayfs directory");
            }
            let upper_tmpfs =
                Filesystem::new(Arc::new(Tmpfs::new())).expect("the instance must stand");
            (
                Overlay::new(vec![lower], Some(upper_tmpfs)),
                format!(
                    "lowerdir={},upperdir={upper},workdir={work},{OVERLAY_FEATURES_OFF}",
                    self.dir("linux-lower")
                ),
            )
        } else {
            let second = Filesystem::new(Arc::new(bind(&self.dir("kobako-second"))))
                .expect("the instance must stand");
            (
                Overlay::new(vec![lower, second], None),
                format!(
                    "lowerdir={}:{}",
                    self.dir("linux-lower"),
                    self.dir("linux-second")
                ),
            )
        };
        let overlay = overlay.unwrap_or_else(|error| panic!("the Overlay must stack: {error:?}"));
        mount("overlay", &root, Some("overlay"), 0, Some(&data));
        let filesystem = Filesystem::new(Arc::new(overlay)).expect("the instance must stand");
        (
            Kobako::new(filesystem, Access::ReadWrite, true),
            Linux::new(&root, true),
        )
    }

    /// Let go of what a finished run left mounted, and hold a lower Layer to
    /// what it started as, as neither side's run may reach one.
    fn finish(self) {
        match self {
            Self::Overlay | Self::OverlayNoUpper => {
                umount(&self.dir("linux-merged"));
                let snapshot = |side: &str| {
                    let mut out = Snapshot::new();
                    snapshot_into(Path::new(&self.dir(side)), "", false, &mut out);
                    out
                };
                assert_eq!(
                    snapshot("kobako-lower"),
                    snapshot("linux-lower"),
                    "a sequence through an Overlay must leave its lower Layer as overlayfs leaves \
                     its own"
                );
            }
            Self::ReadOnlyBind => umount(&self.dir("linux-mount")),
            _ => {}
        }
    }
}

fn bind(dir: &str) -> Bind {
    Bind::new(dir).unwrap_or_else(|error| panic!("{dir} must bind: {error}"))
}

/// A directory holding what a walk meets: files, hard links, directories, and
/// links to a file, to a directory, and to nothing.
fn seed_directory(dir: &str) {
    let _ = fs::remove_dir_all(dir);
    for sub in ["d/e", "m/s"] {
        fs::create_dir_all(format!("{dir}/{sub}")).expect("the seeded directory");
    }
    for (file, content) in [
        ("a", "hello"),
        ("d/f", "inner"),
        ("m/x", "x"),
        ("h1", "hard"),
    ] {
        fs::write(format!("{dir}/{file}"), content).expect("the seeded file");
    }
    fs::hard_link(format!("{dir}/h1"), format!("{dir}/h2")).expect("the seeded hard link");
    for (target, name) in [("a", "l"), ("d", "n"), ("nowhere", "g")] {
        symlink(target, format!("{dir}/{name}")).expect("the seeded link");
    }
}

/// The same tree on a Tmpfs, which takes no seeded directory of the host's.
fn tmpfs_seed() -> Vec<Call> {
    let file = |path: &str, slot| {
        Call::Open(calls::Open {
            path: path.to_string(),
            read: true,
            write: true,
            create: true,
            slot,
            ..calls::Open::default()
        })
    };
    let mkdir = |path: &str| Call::Mkdir(None, path.to_string());
    vec![
        file("a", 0),
        Call::Write(0, 4),
        Call::Close(0),
        mkdir("d"),
        mkdir("d/e"),
        file("d/f", 0),
        Call::Write(0, 2),
        Call::Close(0),
        mkdir("m"),
        mkdir("m/s"),
    ]
}

/// A way one setup differs, by the shortest sequence that reaches it.
struct Difference {
    calls: Vec<Call>,
    detail: String,
}

fn class(call: &Call, kobako: &str, linux: &str) -> String {
    let brief = |answer: &str| {
        let answer = answer.split(':').next().unwrap_or(answer);
        answer.split(" len=").next().unwrap_or(answer).to_string()
    };
    // What a move starts from decides what a difference waives: a link of a lower
    // Layer is kept apart from any other entry refusing to move.
    let subject = match call {
        Call::Rename(_, from, _) => format!(" from={}", from.split('/').next().unwrap_or(from)),
        _ => String::new(),
    };
    format!(
        "{}{subject} kobako={} linux={}",
        call.kind(),
        brief(kobako),
        brief(linux)
    )
}

fn note(found: &mut BTreeMap<String, Difference>, class: String, calls: &[Call], detail: String) {
    let difference = found.entry(class).or_insert_with(|| Difference {
        calls: calls.to_vec(),
        detail: detail.clone(),
    });
    if calls.len() < difference.calls.len() {
        difference.calls = calls.to_vec();
        difference.detail = detail;
    }
}

/// Fail unless what `found` holds is what the expected file names for `name`.
fn assert_named(name: &str, found: &BTreeMap<String, Difference>) {
    let expected: BTreeSet<String> = EXPECTED
        .lines()
        .filter_map(|line| line.split('#').next())
        .filter_map(|line| line.trim().strip_prefix(name))
        .filter_map(|line| line.strip_prefix(' '))
        .map(|line| line.trim().to_string())
        .collect();
    let new: Vec<String> = found
        .iter()
        .filter(|(class, _)| !expected.contains(*class))
        .map(|(class, difference)| {
            let calls: Vec<String> = difference
                .calls
                .iter()
                .map(|call| format!("    {call:?}"))
                .collect();
            format!(
                "{name} {class}\n  {}\n{}",
                difference.detail,
                calls.join("\n")
            )
        })
        .collect();
    let gone: Vec<&String> = expected
        .iter()
        .filter(|class| !found.contains_key(*class))
        .collect();
    assert!(
        new.is_empty() && gone.is_empty(),
        "{name} must answer as Linux does, apart from what linux-baseline-expected.txt names.\nNew \
         differences:\n{}\nNamed but not reached:\n{gone:?}",
        new.join("\n")
    );
}

/// Fail unless what `found` holds is what the expected file names for `setup`,
/// and unless every kind of call this setup can answer ever succeeded.
fn assert_holds(setup: Setup, found: &BTreeMap<String, Difference>, succeeded: &BTreeSet<String>) {
    assert_named(setup.name(), found);
    let mut owed = vec![
        "Open", "Close", "Read", "Pread", "Seek", "Fstat", "Fsync", "Readdir", "Stat",
    ];
    if setup.changes() {
        owed.extend([
            "Write", "Pwrite", "Truncate", "Mkdir", "Rmdir", "Unlink", "Rename",
        ]);
    }
    let missing: Vec<_> = owed
        .into_iter()
        .filter(|kind| !succeeded.contains(*kind))
        .collect();
    assert!(
        missing.is_empty(),
        "{}: no sequence ever made {missing:?} succeed, so agreeing on them shows nothing",
        setup.name()
    );
}

/// Hold `setup` to Linux over `runs` sequences of at most `longest` calls.
fn holds(setup: Setup, runs: u64, longest: u64) {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        on_tmpfs("/dev/shm"),
        "/dev/shm must be a tmpfs, whose answers a Tmpfs is held to; run this through `rake \
         crates:linux_baseline`"
    );
    setup.prepare();
    let paths = setup.paths();
    let mut found = BTreeMap::new();
    let mut succeeded = BTreeSet::new();
    for seed in 1..=runs {
        let mut rng = Rng::new(seed);
        let length = 1 + rng.random_below(longest);
        let sequence: Vec<Call> = (0..length).map(|_| rng.call(&paths)).collect();
        let (mut kobako, mut linux) = setup.start(false);
        let mut agreed = true;
        for (at, call) in sequence.iter().enumerate() {
            let (theirs, ours) = (linux.run(call), kobako.run(call));
            if ours != theirs {
                let detail = format!("kobako={ours} linux={theirs}");
                note(
                    &mut found,
                    class(call, &ours, &theirs),
                    &sequence[..=at],
                    detail,
                );
                agreed = false;
                break;
            }
            if ours.starts_with("ok") || ours.contains("len=") || ours == "dir" {
                succeeded.insert(call.kind().to_string());
            }
        }
        if agreed {
            let (ours, theirs) = (kobako.snapshot(), linux.snapshot());
            if let Some(path) = ours
                .keys()
                .chain(theirs.keys())
                .find(|path| ours.get(*path) != theirs.get(*path))
            {
                let detail = format!(
                    "{path}: kobako={:?} linux={:?}",
                    ours.get(path),
                    theirs.get(path)
                );
                note(&mut found, "Tree".to_string(), &sequence, detail);
            }
        }
        drop((kobako, linux));
        setup.finish();
    }
    assert_holds(setup, &found, &succeeded);
}

/// Which of each entry's two times `after` shows moved since `before`.
fn times_moved<T: PartialEq>(before: &Times<T>, after: &Times<T>) -> BTreeSet<String> {
    let mut moved = BTreeSet::new();
    for (path, (modified, changed)) in after {
        if let Some((was_modified, was_changed)) = before.get(path) {
            if modified != was_modified {
                moved.insert(format!("{path} modified"));
            }
            if changed != was_changed {
                moved.insert(format!("{path} changed"));
            }
        }
    }
    moved
}

/// Hold `setup` to which times each call moves, over `runs` sequences, waiting
/// out the clock around each call. Where the two disagree on an answer, the
/// run that holds answers reports it.
fn moves_the_same_times(setup: Setup, runs: u64, longest: u64) {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    setup.prepare();
    let paths = setup.paths();
    let mut calls_moving_a_time = 0;
    for seed in 1..=runs {
        let mut rng = Rng::new(seed);
        let length = 1 + rng.random_below(longest);
        let sequence: Vec<Call> = (0..length).map(|_| rng.call(&paths)).collect();
        let (mut kobako, mut linux) = setup.start(true);
        for (at, call) in sequence.iter().enumerate() {
            let (kobako_before, linux_before) = (kobako.times(), linux.times());
            sleep(CLOCK_TICK);
            if kobako.run(call) != linux.run(call) {
                break;
            }
            let ours = times_moved(&kobako_before, &kobako.times());
            let theirs = times_moved(&linux_before, &linux.times());
            assert_eq!(
                ours,
                theirs,
                "{} through vfs must move the times Linux moves, after {:#?}",
                setup.name(),
                &sequence[..=at]
            );
            calls_moving_a_time += usize::from(!ours.is_empty());
        }
        drop((kobako, linux));
        setup.finish();
    }
    assert!(
        calls_moving_a_time >= 40,
        "{}: only {calls_moving_a_time} calls moved a time, too few for agreeing to show anything",
        setup.name()
    );
}

/// Hold `setup` to Linux over `rounds` of racing sequences of at most `longest`
/// calls each.
fn holds_racing(setup: Setup, rounds: u64, longest: u64) {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    setup.prepare();
    let mut found = concurrent::Found::default();
    for seed in 1..=rounds {
        let (kobako, linux) = setup.start(false);
        let kobako: Vec<Kobako> = (0..concurrent::THREADS).map(|_| kobako.another()).collect();
        let linux: Vec<Linux> = (0..concurrent::THREADS).map(|_| linux.another()).collect();
        drop(concurrent::round(seed, longest, kobako, linux, &mut found));
        setup.finish();
    }
    assert!(
        found.broken_by_linux.is_empty(),
        "{}: Linux itself broke a rule racing calls are held to, so the rule is wrong: {:#?}",
        setup.name(),
        found.broken_by_linux
    );
    let answers: Vec<_> = found.kobako.difference(&found.linux).collect();
    assert!(
        answers.is_empty() && found.broken_by_kobako.is_empty(),
        "{} racing on one instance must answer only what Linux answers and leave what any \
         interleaving leaves.\nAnswers Linux never gave: {answers:#?}\nBroken at rest: {:#?}",
        setup.name(),
        found.broken_by_kobako
    );
}

#[test]
#[ignore = "needs root and a mount namespace; run by rake crates:linux_baseline"]
fn a_tmpfs_answers_as_a_tmpfs_of_linux() {
    holds(Setup::Tmpfs, 40_000, 30);
}

#[test]
#[ignore = "needs root and a mount namespace; run by rake crates:linux_baseline"]
fn an_overlay_answers_as_overlayfs() {
    holds(Setup::Overlay, 8_000, 30);
}

#[test]
#[ignore = "needs root and a mount namespace; run by rake crates:linux_baseline"]
fn an_overlay_without_an_upper_answers_as_overlayfs() {
    holds(Setup::OverlayNoUpper, 8_000, 20);
}

#[test]
#[ignore = "needs root and a mount namespace; run by rake crates:linux_baseline"]
fn a_bind_answers_as_the_directory_it_binds() {
    holds(Setup::Bind, 15_000, 30);
}

#[test]
#[ignore = "needs root and a mount namespace; run by rake crates:linux_baseline"]
fn a_read_only_bind_answers_as_a_read_only_mount() {
    holds(Setup::ReadOnlyBind, 6_000, 15);
}

#[test]
#[ignore = "needs root and a mount namespace; run by rake crates:linux_baseline"]
fn a_tmpfs_moves_the_times_a_tmpfs_of_linux_moves() {
    moves_the_same_times(Setup::Tmpfs, 600, 6);
}

#[test]
#[ignore = "needs root and a mount namespace; run by rake crates:linux_baseline"]
fn an_overlay_moves_the_times_overlayfs_moves() {
    moves_the_same_times(Setup::Overlay, 600, 6);
}

#[test]
#[ignore = "needs root and a mount namespace; run by rake crates:linux_baseline"]
fn calls_racing_on_a_tmpfs_answer_as_linux() {
    holds_racing(Setup::Tmpfs, 30_000, 40);
}

#[test]
#[ignore = "needs root and a mount namespace; run by rake crates:linux_baseline"]
fn calls_racing_on_an_overlay_answer_as_overlayfs() {
    holds_racing(Setup::Overlay, 30_000, 40);
}

#[test]
#[ignore = "needs root and a mount namespace; run by rake crates:linux_baseline"]
fn calls_racing_on_a_bind_answer_as_linux() {
    holds_racing(Setup::Bind, 30_000, 40);
}
