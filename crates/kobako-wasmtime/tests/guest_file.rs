//! The bundled guest's `File` over a Mount: what a script reads and
//! writes beneath a mounted path, and how it fails where nothing is
//! mounted, nothing is there, or the Mount only reads.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use kobako_runtime::filesystem::Access;
use kobako_runtime::filesystem::{
    DirEntry, File, Filesystem, FilesystemError, Memory, Metadata, OpenOptions, Rename,
};
use kobako_runtime::grant::Grant;
use kobako_runtime::mount::Mount;
use kobako_runtime::Hold;
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
    Grant::Mount(Mount::new("/data", filesystem, access).expect("/data is a guest path"))
}

fn note_memory() -> Arc<Memory> {
    Arc::new([("note.txt", "hello\nworld\n")].into_iter().collect())
}

fn contents(filesystem: &dyn Filesystem, path: &str) -> Vec<u8> {
    let mut file = filesystem
        .open(path, &{
            let mut options = OpenOptions::default();
            options.read = true;
            options
        })
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
        &[mount(note_memory(), Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("hello\nworld\n".to_string()),
        "File.read beneath a Mount through the bundled guest must read the file"
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
        &[mount(note_memory(), Access::Read)],
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
        "File.open with no Mount through the bundled guest must raise \
         NotImplementedError"
    );
}

// @behavior FL-005
#[test]
fn opening_a_missing_file_raises_enoent() {
    let Some(answer) = run(
        r#"File.read("/data/missing.txt")"#,
        &[mount(note_memory(), Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Err("Errno::ENOENT".to_string()),
        "File.read of a path a Mount does not hold through the bundled guest must raise \
         Errno::ENOENT"
    );
}

// @behavior FL-006
#[test]
fn writing_beneath_a_read_only_mount_raises_erofs() {
    let Some(answer) = run(
        r#"File.write("/data/note.txt", "changed")"#,
        &[mount(note_memory(), Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Err("Errno::EROFS".to_string()),
        "File.write beneath a read-only Mount through the bundled guest must raise Errno::EROFS"
    );
}

// The write goes through an open file, so `IO#write` itself must raise,
// not only `File.write`.
// @behavior FL-012
#[test]
fn a_write_past_a_mounts_space_raises_enospc() {
    let full = Arc::new(Memory::new().with_limit(Some(4)));
    let Some(answer) = run(
        r#"File.open("/data/note.txt", "w") { |file| file.write("five!") }"#,
        &[mount(full, Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Err("Errno::ENOSPC".to_string()),
        "IO#write past a Memory's limit through the bundled guest must raise Errno::ENOSPC"
    );
}

// A name at NAME_MAX still lands, so the refusal is the limit and not
// the name's length alone.
// @behavior FL-013
#[test]
fn a_name_or_path_past_linuxs_limits_raises_enametoolong() {
    let script = r#"
        answers = [255, 256].map do |length|
          begin
            File.write("/data/" + "x" * length, "")
            :written
          rescue Errno::ENAMETOOLONG
            :too_long
          end
        end
        begin
          File.read("/data/" + "d/" * 2048)
        rescue Errno::ENAMETOOLONG
          answers << :too_long
        end
        print answers.inspect
    "#;
    let Some(answer) = run(
        script,
        &[mount(Arc::new(Memory::new()), Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("[:written, :too_long, :too_long]".to_string()),
        "a name past 255 bytes or a path past 4095 through the bundled guest's File must raise \
         Errno::ENAMETOOLONG"
    );
}

// EBUSY once had no class in the guest, so the script saw an IOError.
// @behavior FL-014
#[test]
fn a_code_the_engine_answers_raises_its_own_errno() {
    let Some(answer) = run(
        r#"File.rename("/data", "/data/moved")"#,
        &[mount(note_memory(), Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Err("Errno::EBUSY".to_string()),
        "File.rename of a Mount's root through the bundled guest must raise Errno::EBUSY"
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
        &[mount(note_memory(), Access::Read)],
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

// The file keeping its content shows `wx` refused before truncating,
// which a plain `w` would have done.
// @behavior FL-024
#[test]
fn an_exclusive_create_refuses_a_file_already_there() {
    let memory = note_memory();
    let Some(answer) = run(
        r#"File.open("/data/note.txt", "wx") { |f| f.write("lost") }"#,
        &[mount(memory.clone(), Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        (answer, contents(memory.as_ref(), "note.txt")),
        (Err("Errno::EEXIST".to_string()), b"hello\nworld\n".to_vec()),
        "File.open with wx of a file already there through the bundled guest must raise \
         Errno::EEXIST and leave the file as it was"
    );
}

// @behavior FL-025
#[test]
fn an_exclusive_create_makes_a_file_nothing_is_at() {
    let memory = Arc::new(Memory::new());
    let Some(answer) = run(
        r#"File.open("/data/lock", "w+x") { |f| f.write("mine") }; nil"#,
        &[mount(memory.clone(), Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        (answer, contents(memory.as_ref(), "lock")),
        (Ok(String::new()), b"mine".to_vec()),
        "File.open with w+x of a path nothing is at through the bundled guest must create the \
         file with what was written"
    );
}

// `bw` and `wbt` once opened, as `b` and `t` were dropped wherever they
// stood; CRuby refuses both.
// @behavior FL-026
#[test]
fn an_access_mode_cruby_refuses_raises_argument_error() {
    let script = r#"
        answers = %w[xw rx r+x ax bw wbt].map do |mode|
          begin
            File.open("/data/note.txt", mode) { }
            "opened"
          rescue ArgumentError => e
            e.message
          end
        end
        print answers.join(",")
    "#;
    let Some(answer) = run(
        script,
        &[mount(note_memory(), Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok(
            "invalid access mode xw,invalid access mode rx,invalid access mode r+x,\
            invalid access mode ax,invalid access mode bw,invalid access mode wbt"
                .to_string()
        ),
        "File.open with an access mode CRuby refuses through the bundled guest must raise \
         ArgumentError naming the mode"
    );
}

// @behavior FL-027
#[test]
fn an_integer_mode_opens_by_its_file_constants_flags() {
    let memory = note_memory();
    let Some(answer) = run(
        r#"File.open("/data/note.txt", File::WRONLY | File::CREAT | File::EXCL) { }"#,
        &[mount(memory.clone(), Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        (answer, contents(memory.as_ref(), "note.txt")),
        (Err("Errno::EEXIST".to_string()), b"hello\nworld\n".to_vec()),
        "File.open with WRONLY | CREAT | EXCL of a file already there through the bundled guest \
         must raise Errno::EEXIST and leave the file as it was"
    );
}

// EXCL alone asks no create, so the open finds nothing rather than
// making the file an exclusive create would.
// @behavior FL-028
#[test]
fn an_exclusive_open_without_create_makes_no_file() {
    let memory = Arc::new(Memory::new());
    let Some(answer) = run(
        r#"File.open("/data/lock", File::RDONLY | File::EXCL) { }"#,
        &[mount(memory.clone(), Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        (answer, memory.metadata("lock").is_err()),
        (Err("Errno::ENOENT".to_string()), true),
        "File.open with RDONLY | EXCL of a path nothing is at through the bundled guest must \
         raise Errno::ENOENT and make no file"
    );
}

// 0o4000 is CRuby's `NONBLOCK` on Linux, a flag kobako does not honor, so
// ignoring it would open differently than CRuby. `WRONLY | RDWR` names
// no access mode on Linux.
// @behavior FL-029
#[test]
fn an_integer_mode_with_a_flag_file_constants_lacks_raises_einval() {
    let script = r#"
        answers = [File::RDONLY | 0o4000, File::WRONLY | File::RDWR].map do |mode|
          begin
            File.open("/data/note.txt", mode) { }
            "opened"
          rescue => e
            e.class.to_s
          end
        end
        print answers.join(",")
    "#;
    let Some(answer) = run(
        script,
        &[mount(note_memory(), Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("Errno::EINVAL,Errno::EINVAL".to_string()),
        "File.open with a mode bit no File::Constants flag names, or with WRONLY beside RDWR, \
         through the bundled guest must raise Errno::EINVAL"
    );
}

// CRuby's `RDONLY` is zero, so beside `WRONLY` it asks nothing more.
// @behavior FL-031
#[test]
fn rdonly_beside_wronly_opens_to_write_only() {
    let memory = note_memory();
    let script = r#"
        File.open("/data/note.txt", File::RDONLY | File::WRONLY) do |file|
          file.write("J")
          begin
            file.read
          rescue IOError => e
            print e.class
          end
        end
    "#;
    let Some(answer) = run(
        script,
        &[mount(memory.clone(), Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        (answer, contents(memory.as_ref(), "note.txt")),
        (Ok("IOError".to_string()), b"Jello\nworld\n".to_vec()),
        "File.open with RDONLY | WRONLY through the bundled guest must write and raise IOError \
         on a read"
    );
}

// The values are CRuby's on Linux, so a script built from numbers it saw
// there opens the same way.
// @behavior FL-030
#[test]
fn file_constants_names_the_flags_an_open_honors() {
    let script = r##"
        names = File::Constants.constants.sort
        print names.map { |name| "#{name}=#{File::Constants.const_get(name)}" }.join(","),
              " ", IO.include?(File::Constants)
    "##;
    let Some(answer) = run(script, &[], Vec::new()) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("APPEND=1024,CREAT=64,EXCL=128,RDONLY=0,RDWR=2,TRUNC=512,WRONLY=1 true".to_string()),
        "File::Constants in the bundled guest must name the seven flags an open honors at \
         CRuby's Linux values, and IO must include it"
    );
}

/// A Memory whose every open file is handed through `wrap` first.
struct WrapsFiles<W> {
    memory: Memory,
    wrap: W,
}

impl<W> WrapsFiles<W> {
    fn over_note(contents: &str, wrap: W) -> Arc<Self> {
        Arc::new(Self {
            memory: [("note.txt", contents)].into_iter().collect(),
            wrap,
        })
    }
}

impl<W, F> Filesystem for WrapsFiles<W>
where
    W: Fn(Box<dyn File>) -> F + Send + Sync,
    F: File + 'static,
{
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        Ok(Box::new((self.wrap)(self.memory.open(path, options)?)))
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        self.memory.metadata(path)
    }

    fn symlink_metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        self.memory.symlink_metadata(path)
    }

    fn stored_path(&self, path: &str) -> Result<String, FilesystemError> {
        self.memory.stored_path(path)
    }

    fn canonicalize_beneath(&self, base: &str, path: &str) -> Result<String, FilesystemError> {
        self.memory.canonicalize_beneath(base, path)
    }

    fn for_policy(&self) -> Option<Arc<dyn Filesystem>> {
        self.memory.for_policy()
    }

    fn hold_directory(&self, path: &str) -> Result<Option<Hold>, FilesystemError> {
        self.memory.hold_directory(path)
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        self.memory.read_dir(path)
    }

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.memory.create_dir(path)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.memory.remove_dir(path)
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        self.memory.remove_file(path)
    }

    fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
        self.memory.rename(from, to, rename)
    }

    fn for_invocation(&self) -> Option<Arc<dyn Filesystem>> {
        self.memory.for_invocation()
    }
}

struct CountedFile {
    file: Box<dyn File>,
    reads: Arc<AtomicUsize>,
}

impl File for CountedFile {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.file.read_at(buf, offset)
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.file.write_at(buf, offset)
    }

    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        self.file.append(buf)
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        self.file.metadata()
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        self.file.set_len(len)
    }
}

/// Each read reaching the Filesystem crosses the wasm boundary, and one a
/// Host App writes in Ruby re-enters Ruby too, so a read of a length the
/// file holds asks once.
#[test]
fn a_read_of_a_length_asks_the_filesystem_once() {
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = reads.clone();
    let filesystem = WrapsFiles::over_note(&"x".repeat(6400), move |file| CountedFile {
        file,
        reads: counted.clone(),
    });
    let Some(answer) = run(
        r#"File.open("/data/note.txt") { |f| print f.read(64).size }; nil"#,
        &[mount(filesystem, Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(answer, Ok("64".to_string()));
    assert_eq!(
        reads.load(Ordering::Relaxed),
        1,
        "File#read(64) of a longer file through the bundled guest must ask its Filesystem for one read"
    );
}

/// An open file whose Filesystem refuses to release it, as a database
/// refusing the commit a close stands for.
struct RefusesClose(Box<dyn File>);

impl File for RefusesClose {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        self.0.read_at(buf, offset)
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.0.write_at(buf, offset)
    }

    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        self.0.append(buf)
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        self.0.metadata()
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        self.0.set_len(len)
    }

    fn close(self: Box<Self>) -> Result<(), FilesystemError> {
        Err(FilesystemError::PermissionDenied)
    }
}

/// An open file whose every read fails in a way no error code names.
struct FailsReads(Box<dyn File>);

impl File for FailsReads {
    fn read_at(&mut self, _buf: &mut [u8], _offset: u64) -> Result<usize, FilesystemError> {
        Err(FilesystemError::Other("the disk went away".to_string()))
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.0.write_at(buf, offset)
    }

    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        self.0.append(buf)
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        self.0.metadata()
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        self.0.set_len(len)
    }
}

// @behavior FL-011
#[test]
fn a_failure_no_error_code_names_raises_eio() {
    let filesystem = WrapsFiles::over_note("hello", FailsReads);
    let Some(answer) = run(
        r#"begin; File.read("/data/note.txt"); rescue Errno::EIO; print "failed"; end; nil"#,
        &[mount(filesystem, Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("failed".to_string()),
        "File.read of a file whose Filesystem fails with no named code through the bundled guest must raise Errno::EIO"
    );
}

// @behavior FL-010
#[test]
fn a_close_the_filesystem_refuses_fails_the_scripts_close() {
    let filesystem = WrapsFiles::over_note("hello", RefusesClose);
    let Some(answer) = run(
        r#"f = File.open("/data/note.txt"); begin; f.close; rescue Errno::EACCES; print "refused"; end; print f.closed?; nil"#,
        &[mount(filesystem, Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("refusedtrue".to_string()),
        "File#close of a file whose Filesystem refuses its close through the bundled guest must raise Errno::EACCES and leave the file closed"
    );
}

/// An open file taking at most three bytes per write, as a Filesystem a
/// Host App writes in Ruby may.
struct WritesInParts(Box<dyn File>);

impl File for WritesInParts {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        self.0.read_at(buf, offset)
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.0.write_at(&buf[..buf.len().min(3)], offset)
    }

    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        self.0.append(&buf[..buf.len().min(3)])
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        self.0.metadata()
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        self.0.set_len(len)
    }
}

// @behavior FL-016
#[test]
fn an_errno_a_script_raises_reads_as_crubys() {
    let script = r#"
        out = []
        begin; raise Errno::ENOENT, "foo"; rescue => e; out << e.message; end
        out << Errno::ENOENT.new.message
        e = SystemCallError.new("failed", 2)
        out << e.class.to_s << e.errno
        print out.inspect
    "#;
    let Some(answer) = run(script, &[mount(note_memory(), Access::Read)], Vec::new()) else {
        return;
    };
    assert_eq!(
        answer,
        Ok(r#"["No such file or directory - foo", "No such file or directory", "Errno::ENOENT", 2]"#
            .to_string()),
        "Errno::ENOENT raised with a message, built bare, and SystemCallError.new with a known \
         number through the bundled guest must read as CRuby's"
    );
}

// No Errno class names 9999, so only the number the error was built with
// can answer `errno`.
// @behavior FL-019
#[test]
fn a_system_call_error_keeps_a_number_no_class_names() {
    let Some(answer) = run(
        r#"e = SystemCallError.new(9999); print [e.class.to_s, e.message, e.errno].inspect"#,
        &[mount(note_memory(), Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok(r#"["SystemCallError", "Unknown error: 9999", 9999]"#.to_string()),
        "SystemCallError.new with a number no Errno class names through the bundled guest must \
         keep that number as its errno, as CRuby does"
    );
}

// @behavior FL-022
#[test]
fn a_read_length_past_64_bits_raises_range_error() {
    let script = r#"
        begin
          File.open("/data/note.txt") { |f| f.read(2**70) }
        rescue RangeError => e
          print e.class.to_s
        end
    "#;
    let Some(answer) = run(script, &[mount(note_memory(), Access::Read)], Vec::new()) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("RangeError".to_string()),
        "File#read with a length of 2**70 through the bundled guest must raise RangeError, as \
         CRuby does"
    );
}

// The C errno is an `int`, so 2**40 is already past it although it is no
// big integer.
// @behavior FL-023
#[test]
fn a_system_call_error_number_past_the_errno_range_raises_range_error() {
    let script = r#"
        out = [2**40, 2**70].map do |number|
          SystemCallError.new("failed", number)
        rescue RangeError => e
          e.class.to_s
        end
        print out.inspect
    "#;
    let Some(answer) = run(script, &[mount(note_memory(), Access::Read)], Vec::new()) else {
        return;
    };
    assert_eq!(
        answer,
        Ok(r#"["RangeError", "RangeError"]"#.to_string()),
        "SystemCallError.new with a message and 2**40 or 2**70 through the bundled guest must \
         raise RangeError, as CRuby does"
    );
}

// @behavior FL-017
#[test]
fn a_read_or_write_a_stream_was_not_opened_for_raises_ioerror() {
    let script = r#"
        out = []
        [-> { File.open("/data/note.txt", "r") { |f| f.write("x") } },
         -> { File.open("/data/note.txt", "w") { |f| f.read } },
         -> { $stdout.read }].each do |call|
          begin; call.call; rescue IOError => e; out << e.message; end
        end
        print out.inspect
    "#;
    let Some(answer) = run(
        script,
        &[mount(note_memory(), Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok(
            r#"["not opened for writing", "not opened for reading", "not opened for reading"]"#
                .to_string()
        ),
        "a write to a file opened with r, a read of one opened with w, and $stdout.read through \
         the bundled guest must raise IOError as CRuby does"
    );
}

// @behavior FL-018
#[test]
fn a_path_that_is_no_valid_utf8_raises_eilseq() {
    let Some(answer) = run(
        r#"File.read("/data/\xff")"#,
        &[mount(note_memory(), Access::Read)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Err("Errno::EILSEQ".to_string()),
        "File.read of a path holding the byte 0xff through the bundled guest must raise Errno::EILSEQ"
    );
}

// @behavior FL-015
#[test]
fn a_write_lands_whole_when_the_filesystem_takes_it_in_parts() {
    let filesystem = WrapsFiles::over_note("", WritesInParts);
    let Some(answer) = run(
        r#"File.open("/data/note.txt", "w") { |f| print f.write("hello world") }; nil"#,
        &[mount(filesystem.clone(), Access::ReadWrite)],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        (answer, contents(filesystem.as_ref(), "note.txt")),
        (Ok("11".to_string()), b"hello world".to_vec()),
        "IO#write to a file whose Filesystem takes three bytes per write through the bundled guest \
         must answer every byte and land them all"
    );
}

/// An open file reporting a length it does not hold, so a script meets a
/// file of several GiB without the test allocating one.
struct ReportsLen(Box<dyn File>, u64);

impl File for ReportsLen {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        self.0.read_at(buf, offset)
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.0.write_at(buf, offset)
    }

    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        self.0.append(buf)
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        let mut metadata = self.0.metadata()?;
        metadata.len = self.1;
        Ok(metadata)
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        self.0.set_len(len)
    }
}

fn three_gib_note() -> Grant {
    let filesystem = WrapsFiles::over_note("hello", |file| ReportsLen(file, 3 << 30));
    mount(filesystem, Access::Read)
}

// @behavior FL-020
#[test]
fn a_file_past_2_gib_reports_its_whole_size() {
    let Some(answer) = run(
        r#"File.open("/data/note.txt") { |f| print f.size }"#,
        &[three_gib_note()],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("3221225472".to_string()),
        "File#size of a 3 GiB file through the bundled guest must answer 3221225472, not a \
         32-bit ceiling"
    );
}

// @behavior FL-021
#[test]
fn a_position_past_2_gib_is_reported_whole() {
    let Some(answer) = run(
        r#"File.open("/data/note.txt") { |f| f.seek(2147483649); print f.pos }"#,
        &[three_gib_note()],
        Vec::new(),
    ) else {
        return;
    };
    assert_eq!(
        answer,
        Ok("2147483649".to_string()),
        "IO#pos after seeking a 3 GiB file to 2 GiB and one byte through the bundled guest \
         must answer 2147483649"
    );
}
