//! The bundled guest's `File` over a filesystem Grant: what a script reads
//! and writes beneath a mounted path, and how it fails where nothing is
//! mounted, nothing is there, or the Grant only reads.

mod support;

use std::sync::Arc;

use kobako_runtime::filesystem::{Filesystem, Memory, OpenOptions};
use kobako_runtime::grant::{Access, Grant};
use kobako_runtime::{Completion, Entry, Runtime};
use kobako_transport::envelope::{Flag, Outcome, Preamble};

/// What one script left: its printed output, or the class of the
/// exception it ended with. `None` when the Guest Binary is not built.
fn run(source: &str, grants: &[Grant], flags: Vec<Flag>) -> Option<Result<String, String>> {
    let driver = support::bundled_driver()?;
    let mut preamble = Preamble::default();
    preamble.flags = flags;
    let snapshot = driver
        .invoke(
            Entry::Eval {
                source: source.as_bytes(),
            },
            &preamble,
            None,
            grants,
        )
        .expect("the invocation must start");
    let Completion::Outcome(outcome) = snapshot.completion else {
        panic!("the invocation must answer rather than trap");
    };
    Some(match outcome {
        Outcome::Ok(_) => Ok(String::from_utf8(snapshot.stdout.bytes).expect("stdout is UTF-8")),
        Outcome::Panic(panic) => Err(panic.error.name),
        Outcome::Unsupported(_) => panic!("the bundled guest serves eval"),
    })
}

fn mount(filesystem: Arc<dyn Filesystem>, access: Access) -> Grant {
    Grant::Filesystem {
        at: "/data".to_string(),
        access,
        filesystem,
        policy: None,
    }
}

fn seeded() -> Arc<Memory> {
    Arc::new([("note.txt", "hello\nworld\n")].into_iter().collect())
}

fn contents(filesystem: &dyn Filesystem, path: &str) -> Vec<u8> {
    let mut file = filesystem
        .open(
            path,
            &OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        )
        .expect("the file must exist");
    let mut buf = vec![0; 64];
    let read = file.read_at(&mut buf, 0).expect("the file must read");
    buf.truncate(read);
    buf
}

// @behavior FL-001
#[test]
fn a_script_reads_a_file_beneath_a_mounted_path() {
    let Some(answer) = run(
        r#"print File.read("/data/note.txt")"#,
        &[mount(seeded(), Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("hello\nworld\n".to_string()),
        "File.read beneath a filesystem Grant through the bundled guest must read the file"
    );
}

// @behavior FL-002
#[test]
fn a_script_writes_a_file_beneath_a_read_write_mount() {
    let memory = Arc::new(Memory::new());
    let Some(answer) = run(
        r#"print File.open("/data/out.txt", "w") { |f| f.write("kept"); :done }"#,
        &[mount(memory.clone(), Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("done".to_string()),
        "File.open must answer its block's value"
    );
    assert_eq!(
        contents(memory.as_ref(), "out.txt"),
        b"kept",
        "a file written in File.open's block through the bundled guest must land in the Memory"
    );
}

// @behavior FL-003
#[test]
fn a_script_reads_a_file_line_by_line() {
    let Some(answer) = run(
        r#"File.open("/data/note.txt") { |f| f.each_line { |line| print line.upcase } }; nil"#,
        &[mount(seeded(), Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("HELLO\nWORLD\n".to_string()),
        "IO#each_line on a File through the bundled guest must yield each line"
    );
}

// @behavior FL-004
#[test]
fn opening_a_file_with_nothing_mounted_is_unimplemented() {
    let Some(answer) = run(r#"File.open("/data/note.txt")"#, &[], Vec::new()) else {
        return;
    };
    assert_eq!(
        answer,
        Err("NotImplementedError".to_string()),
        "File.open with no filesystem Grant through the bundled guest must raise \
         NotImplementedError"
    );
}

// @behavior FL-005
#[test]
fn opening_a_missing_file_raises_enoent() {
    let Some(answer) = run(
        r#"File.read("/data/missing.txt")"#,
        &[mount(seeded(), Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Err("Errno::ENOENT".to_string()),
        "File.read of a path a Grant does not hold through the bundled guest must raise \
         Errno::ENOENT"
    );
}

// @behavior FL-006
#[test]
fn writing_beneath_a_read_only_mount_raises_erofs() {
    let Some(answer) = run(
        r#"File.write("/data/note.txt", "changed")"#,
        &[mount(seeded(), Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Err("Errno::EROFS".to_string()),
        "File.write beneath a read-only Grant through the bundled guest must raise Errno::EROFS"
    );
}

// @behavior FL-007
#[test]
fn a_file_is_an_io() {
    let Some(answer) = run(r#"print(File.superclass == IO)"#, &[], Vec::new()) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("true".to_string()),
        "File in the bundled guest must descend from IO"
    );
}

// @behavior FL-008
#[test]
fn the_disable_file_flag_removes_file() {
    let flag = Flag {
        name: "disable_file".into(),
        value: String::new(),
        critical: true,
    };
    let Some(answer) = run(r#"print Object.const_defined?(:File)"#, &[], vec![flag]) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("false".to_string()),
        "the disable_file flag through the bundled guest must leave no File constant"
    );
}

// @behavior FL-009
#[test]
fn an_io_may_stand_for_a_file_the_guest_opened() {
    let Some(answer) = run(
        r#"File.open("/data/note.txt") { |f| print IO.new(f.fileno, "r").read }; nil"#,
        &[mount(seeded(), Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("hello\nworld\n".to_string()),
        "IO.new over a file the guest opened through the bundled guest must read that file"
    );
}
