//! Holds what a caller is refused to the Linux that refuses it: a Policy to the
//! same rules under Landlock, and a Bind to a process of another user.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, PoisonError};
use std::thread;

use kobako_runtime::filesystem::Filesystem;
use kobako_runtime::mount::{Access, MountRequest};
use kobako_runtime::policy::Policy;

use crate::calls::{Call, Rng};
use crate::kobako::Kobako;
use crate::linux::Linux;
use crate::{assert_named, bind, class, note, seed_directory, Difference, Setup, BASE, SERIAL};

/// Landlock's right to read a file and to list a directory.
const LANDLOCK_READ: u64 = (1 << 2) | (1 << 3);
/// Every right Landlock's filesystem rules name.
const LANDLOCK_ALL: u64 = (1 << 16) - 1;

const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
const SYS_LANDLOCK_ADD_RULE: libc::c_long = 445;
const SYS_LANDLOCK_RESTRICT_SELF: libc::c_long = 446;
const LANDLOCK_RULE_PATH_BENEATH: libc::c_long = 1;

#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
    handled_access_net: u64,
    scoped: u64,
}

#[repr(C, packed)]
struct PathBeneath {
    allowed_access: u64,
    parent_fd: i32,
}

/// Reads anywhere, and changes only beneath `d`.
struct WritesBeneathD;

impl Policy<MountRequest<'_>> for WritesBeneathD {
    fn allows(&self, request: &MountRequest<'_>) -> bool {
        request.access == Access::Read || request.path.starts_with("/baseline/d/")
    }
}

/// Hold this thread to the rule `WritesBeneathD` states, over `root`, and leave it
/// `unjudged` to do anything, as kobako's own host directory is no guest's.
fn restrict_as_writes_beneath_d(root: &str, unjudged: &str) {
    let attr = RulesetAttr {
        handled_access_fs: LANDLOCK_ALL,
        handled_access_net: 0,
        scoped: 0,
    };
    // SAFETY: each pointer names a live value of the size its call is told, and
    // each descriptor is one this call opened.
    unsafe {
        assert_eq!(libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        let size = size_of::<RulesetAttr>();
        let ruleset = libc::syscall(SYS_LANDLOCK_CREATE_RULESET, &raw const attr, size, 0);
        assert!(
            ruleset >= 0,
            "this kernel must enforce Landlock: {}",
            io::Error::last_os_error()
        );
        let rules = [
            (root.to_string(), LANDLOCK_READ),
            (format!("{root}/d"), LANDLOCK_ALL),
            (unjudged.to_string(), LANDLOCK_ALL),
        ];
        for (path, allowed_access) in rules {
            let path = CString::new(path).expect("no path holds a NUL");
            let parent_fd = libc::open(path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC);
            assert!(parent_fd >= 0, "{}", io::Error::last_os_error());
            let rule = PathBeneath {
                allowed_access,
                parent_fd,
            };
            let added = libc::syscall(
                SYS_LANDLOCK_ADD_RULE,
                ruleset,
                LANDLOCK_RULE_PATH_BENEATH,
                &raw const rule,
                0,
            );
            assert_eq!(added, 0, "{}", io::Error::last_os_error());
            libc::close(parent_fd);
        }
        assert_eq!(libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset, 0), 0);
        libc::close(ruleset as i32);
    }
}

/// Run `sequence` through both sides until they disagree, as `differs` says of
/// two answers, noting what it does.
fn compare(
    sequence: &[Call],
    mut kobako: Kobako,
    mut linux: Linux,
    differs: impl Fn(&str, &str) -> bool,
    found: &mut BTreeMap<String, Difference>,
) {
    for (at, call) in sequence.iter().enumerate() {
        let (theirs, ours) = (linux.run(call), kobako.run(call));
        if differs(&ours, &theirs) {
            let detail = format!("kobako={ours} linux={theirs}");
            note(found, class(call, &ours, &theirs), &sequence[..=at], detail);
            return;
        }
    }
}

fn sequence(seed: u64, longest: u64, paths: &[String]) -> Vec<Call> {
    let mut rng = Rng::new(seed);
    let length = 1 + rng.random_below(longest);
    (0..length).map(|_| rng.call(paths)).collect()
}

#[test]
#[ignore = "needs root and a mount namespace; run by rake crates:linux_baseline"]
fn a_policy_answers_as_the_same_rules_under_landlock() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let setup = Setup::Bind;
    setup.prepare();
    let paths = setup.paths();
    let mut found = BTreeMap::new();
    for seed in 1..=15_000 {
        let (root, kobako_dir) = (setup.dir("linux"), setup.dir("kobako"));
        seed_directory(&kobako_dir);
        seed_directory(&root);
        let filesystem =
            Filesystem::new(Arc::new(bind(&kobako_dir))).expect("the instance must stand");
        let kobako = Kobako::with_policy(filesystem, Arc::new(WritesBeneathD));
        let linux = Linux::new(&root, false);
        let calls = sequence(seed, 30, &paths);
        let found = &mut found;
        thread::scope(|scope| {
            scope
                .spawn(|| {
                    restrict_as_writes_beneath_d(&root, &kobako_dir);
                    compare(&calls, kobako, linux, |ours, theirs| ours != theirs, found);
                })
                .join()
                .expect("a sequence must not panic");
        });
    }
    assert_named("policy", &found);
}

/// A tree another user meets refused in each way a mode refuses: a directory it
/// may only read, search, or not reach, and files it may only read, only write,
/// or not open.
fn seed_permissions(dir: &str) {
    let mode = |path: String, mode| {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("the mode must be set");
    };
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).expect("the host directory");
    mode(dir.to_string(), 0o777);
    for (name, bits) in [
        ("ro", 0o755),
        ("nox", 0o704),
        ("nor", 0o701),
        ("no", 0o700),
        ("w", 0o777),
    ] {
        let sub = format!("{dir}/{name}");
        fs::create_dir(&sub).expect("the host directory");
        fs::write(format!("{sub}/f"), "x").expect("the host file");
        mode(format!("{sub}/f"), 0o666);
        mode(sub, bits);
    }
    for (name, bits) in [("fro", 0o644), ("fwo", 0o602), ("fno", 0o600)] {
        fs::write(format!("{dir}/{name}"), "x").expect("the host file");
        mode(format!("{dir}/{name}"), bits);
    }
}

const PERMISSION_PATHS: [&str; 22] = [
    ".", "x", "ro", "ro/", "ro/f", "ro/x", "nox", "nox/", "nox/f", "nox/x", "nor", "nor/", "nor/f",
    "nor/x", "no", "no/", "no/f", "fro", "fwo", "fno", "w/f", "w/x",
];

/// Whether one side succeeded where the other was refused. kobako has no layer
/// of permissions, so which refusal answers first is the host's to reach, and two
/// refusals of one call agree whatever each names.
fn one_succeeded(ours: &str, theirs: &str) -> bool {
    let refused = |answer: &str| answer.starts_with('E');
    refused(ours) != refused(theirs)
}

#[test]
#[ignore = "needs root and a mount namespace; run by rake crates:linux_baseline"]
fn a_bind_refuses_another_user_as_its_host_does() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let setup = Setup::Bind;
    setup.prepare();
    fs::set_permissions(BASE, fs::Permissions::from_mode(0o755)).expect("the mode must be set");
    let paths = PERMISSION_PATHS.map(str::to_string);
    let mut found = BTreeMap::new();
    for seed in 1..=12_000 {
        let (root, kobako_dir) = (setup.dir("linux"), setup.dir("kobako"));
        seed_permissions(&kobako_dir);
        seed_permissions(&root);
        let filesystem =
            Filesystem::new(Arc::new(bind(&kobako_dir))).expect("the instance must stand");
        let calls = sequence(seed, 20, &paths);
        let found = &mut found;
        thread::scope(|scope| {
            scope
                .spawn(|| {
                    // SAFETY: these change only this thread's credentials, as raw
                    // syscalls do where libc's wrappers would change every thread's.
                    unsafe {
                        assert_eq!(
                            libc::syscall(libc::SYS_setgroups, 0, std::ptr::null::<u32>()),
                            0
                        );
                        assert_eq!(libc::syscall(libc::SYS_setresgid, 1000, 1000, 1000), 0);
                        assert_eq!(libc::syscall(libc::SYS_setresuid, 1000, 1000, 1000), 0);
                    }
                    let kobako = Kobako::new(filesystem, Access::ReadWrite, false);
                    let linux = Linux::new(&root, false);
                    compare(&calls, kobako, linux, one_succeeded, found);
                })
                .join()
                .expect("a sequence must not panic");
        });
    }
    assert_named("another-user", &found);
}
