# File

What a script reaches through the default Guest Binary's `File`, beneath the paths a Mount places.

```
script ─ File / IO ─> wasi-libc ─ preopen at the Mount's path ─> runtime ─> Filesystem
```

### Why these scenarios

`File` is the guest's half of a Mount. It ships in the default Guest Binary, so a script always names it. What it reaches is the Mount's to decide. With no Mount it reaches nothing, and says so as a call the platform lacks.

The failures are CRuby's classes, so a script handles them as it would on any host. A flag removes `File` outright for a Host App that wants no file surface at all.

```
nothing mounted   -> NotImplementedError
nothing at path   -> Errno::ENOENT
read-only Mount   -> Errno::EROFS on a write
disable_file flag -> no File constant
```

## Includes

- `crates/kobako-wasmtime/tests/integration/guest_file.rs`
- `test/parity/test_mount.rb`
- `test/e2e/test_file_open_matrix.rb`

## `FL-001` A script reads a file beneath a mounted path

| Attribute | Value |
| --- | --- |
| relates | `RT-076` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount at `/data` over a file |
| When | the script reads that file with `File.read` |
| Then | it reads the file's content |

## `FL-002` A script writes a file beneath a read-write mount

| Attribute | Value |
| --- | --- |
| relates | `RT-077` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount over a Tmpfs |
| When | the script writes a file inside a `File.open` block |
| Then | the Tmpfs holds that file with what was written |

## `FL-003` A script reads a file line by line

| Attribute | Value |
| --- | --- |
| relates | `FL-001` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount over a file of two lines |
| When | the script reads it with `each_line` |
| Then | the block receives each line in order |

## `FL-004` Opening a file with nothing mounted is unimplemented

| Step | Statement |
| --- | --- |
| Given | an invocation handed no Mount |
| When | the script opens a file |
| Then | the script raises `NotImplementedError` |

## `FL-005` Opening a file a Mount does not hold raises ENOENT

| Attribute | Value |
| --- | --- |
| relates | `FL-001` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the script reads a path nothing is at |
| Then | the script raises `Errno::ENOENT` |

## `FL-006` Writing beneath a read-only mount raises EROFS

| Attribute | Value |
| --- | --- |
| refines | `RT-078` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-only Mount |
| When | the script writes a file beneath it |
| Then | the script raises `Errno::EROFS` |

## `FL-007` A File is an IO

| Step | Statement |
| --- | --- |
| Given | the default Guest Binary |
| When | the script reads the superclass of `File` |
| Then | it is `IO` |

## `FL-008` The disable_file flag removes File

| Attribute | Value |
| --- | --- |
| relates | `FL-004`, `MR-017` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox that set the critical flag `disable_file` |
| When | the script asks whether `File` is defined |
| Then | it is not |

## `FL-009` An IO may stand for a file the guest opened

| Attribute | Value |
| --- | --- |
| refines | `IO-001` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file it opened beneath a Mount |
| When | it constructs an IO over that file's descriptor and reads it |
| Then | it reads the file's content |

## `FL-010` A close the Filesystem refuses fails the script's close

| Attribute | Value |
| --- | --- |
| relates | `FL-005` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file whose Filesystem refuses its close as permission denied |
| When | the script closes that file |
| Then | the close raises `Errno::EACCES` and the file is closed |

## `FL-011` A failure no error code names raises EIO

| Attribute | Value |
| --- | --- |
| relates | `FL-005` |

| Step | Statement |
| --- | --- |
| Given | a script reading a file whose Filesystem fails with no named error code |
| When | the read fails |
| Then | it raises `Errno::EIO` |

## `FL-012` A write past a Mount's space raises ENOSPC

| Attribute | Value |
| --- | --- |
| refines | `FS-022` |
| relates | `FL-011` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount over a Tmpfs with a small limit |
| When | the script writes past that limit through an open file |
| Then | the write raises `Errno::ENOSPC` |

## `FL-013` A name or path past Linux's limits raises ENAMETOOLONG

| Attribute | Value |
| --- | --- |
| relates | `RT-076` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the script names a file over 255 bytes, or a path of 4096 bytes |
| Then | the call raises `Errno::ENAMETOOLONG` |

## `FL-014` A code the engine answers raises its own Errno

| Attribute | Value |
| --- | --- |
| relates | `FL-005`, `RF-015` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount at `/data` |
| When | the script renames `/data` into itself |
| Then | the rename raises `Errno::EBUSY` |

## `FL-015` A write lands whole when the Filesystem takes it in parts

| Attribute | Value |
| --- | --- |
| refines | `FL-002` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount whose files take at most three bytes per write |
| When | the script writes `hello world` to a file with `IO#write` |
| Then | the write answers 11, and the file holds `hello world` |

## `FL-016` An Errno a script raises reads as CRuby's

| Attribute | Value |
| --- | --- |
| relates | `FL-005` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the script raises `Errno::ENOENT` with `foo`, builds one bare, and builds `SystemCallError` with `failed` and 2 |
| Then | the messages read `No such file or directory - foo`, `No such file or directory`, and the last is an `Errno::ENOENT` whose `errno` is 2 |

## `FL-017` A read or write a stream was not opened for raises IOError

| Attribute | Value |
| --- | --- |
| relates | `FL-007` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the script writes to a file opened with `r`, reads one opened with `w`, and reads `$stdout` |
| Then | each raises `IOError`, saying the stream was not opened for writing or for reading |

## `FL-018` A path that is no valid UTF-8 raises EILSEQ

| Attribute | Value |
| --- | --- |
| relates | `FL-013` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the script reads a path holding the byte `0xff` |
| Then | the read raises `Errno::EILSEQ` |

## `FL-019` A SystemCallError keeps a number no class names

| Attribute | Value |
| --- | --- |
| refines | `FL-016` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the script builds `SystemCallError` with 9999 |
| Then | it reads `Unknown error: 9999`, and its `errno` is 9999 |

## `FL-020` A file past 2 GiB reports its whole size

| Attribute | Value |
| --- | --- |
| relates | `FL-001` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file of 3 GiB |
| When | the script asks the file for its size |
| Then | it answers 3 GiB |

## `FL-021` A position past 2 GiB is reported whole

| Attribute | Value |
| --- | --- |
| relates | `FL-020` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file of 3 GiB |
| When | the script seeks to 2 GiB and one more byte and reads its position |
| Then | it answers 2 GiB and one more byte |

## `FL-022` A read length past 64 bits raises RangeError

| Attribute | Value |
| --- | --- |
| relates | `FL-001` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file |
| When | the script reads the file by a length past 64 bits |
| Then | the read raises `RangeError` |

## `FL-023` A SystemCallError number past the errno range raises RangeError

| Attribute | Value |
| --- | --- |
| refines | `FL-019` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the script builds `SystemCallError` with a message and 2⁴⁰, then with a message and 2⁷⁰ |
| Then | each raises `RangeError` |

## `FL-024` An exclusive create refuses a file already there

| Attribute | Value |
| --- | --- |
| refines | `FL-002` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount holding a file |
| When | the script opens that file with `wx` |
| Then | the open raises `Errno::EEXIST`, and the file keeps its content |

## `FL-025` An exclusive create makes a file nothing is at

| Attribute | Value |
| --- | --- |
| refines | `FL-002` |
| relates | `FL-024` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount over a Tmpfs |
| When | the script opens a new file with `w+x` and writes to it |
| Then | the Tmpfs holds that file with what was written |

## `FL-026` An access mode CRuby refuses raises ArgumentError

| Attribute | Value |
| --- | --- |
| relates | `FL-024` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the script opens a file with `xw`, `rx`, `r+x`, `ax`, `bw`, or `wbt` |
| Then | each open raises `ArgumentError` |

## `FL-027` An Integer mode opens by its File::Constants flags

| Attribute | Value |
| --- | --- |
| relates | `FL-024` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount holding a file |
| When | the script opens that file with `File::WRONLY \| File::CREAT \| File::EXCL` |
| Then | the open raises `Errno::EEXIST` |

## `FL-028` An exclusive open without create makes no file

| Attribute | Value |
| --- | --- |
| refines | `FL-027` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the script opens a path nothing is at with `File::RDONLY \| File::EXCL` |
| Then | the open raises `Errno::ENOENT` |

## `FL-029` An Integer mode with a flag File::Constants lacks raises EINVAL

| Attribute | Value |
| --- | --- |
| refines | `FL-027` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the script opens a file with a mode holding a bit no flag names, or `WRONLY` beside `RDWR` |
| Then | the open raises `Errno::EINVAL` |

## `FL-030` File::Constants names the flags an open honors

| Attribute | Value |
| --- | --- |
| relates | `FL-027`, `FL-007` |

| Step | Statement |
| --- | --- |
| Given | the default Guest Binary |
| When | the script lists `File::Constants` and asks whether `IO` includes it |
| Then | it lists `RDONLY`, `WRONLY`, `RDWR`, `CREAT`, `EXCL`, `TRUNC`, and `APPEND` at CRuby's Linux values, and `IO` includes it |

## `FL-031` RDONLY beside WRONLY opens to write only

| Attribute | Value |
| --- | --- |
| refines | `FL-027` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount holding a file |
| When | the script opens that file with `File::RDONLY \| File::WRONLY`, writes, then reads |
| Then | the write lands, and the read raises `IOError` |

## `FL-032` An Integer mode may create or truncate without asking to write

| Attribute | Value |
| --- | --- |
| refines | `FL-027` |
| relates | `RT-146` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount over a Tmpfs holding a file |
| When | the script opens a new path with `File::CREAT \| File::EXCL`, and the file with `File::RDONLY \| File::TRUNC` |
| Then | the Tmpfs holds the new empty file, and the file is empty |

## `FL-033` A path holding a NUL byte raises ArgumentError

| Attribute | Value |
| --- | --- |
| relates | `FL-018` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the script opens a path holding a NUL byte |
| Then | the open raises `ArgumentError` |

## `FL-034` An open answers as CRuby does on Linux, unless a rule refuses it

| Attribute | Value |
| --- | --- |
| relates | `FL-027`, `IO-040`, `RT-147` |

| Step | Statement |
| --- | --- |
| Given | each Mount kind, and CRuby over a Linux directory holding the same entries |
| When | the script opens each entry with each mode, then reads, writes, and wraps it with `IO.new` |
| Then | each answer is CRuby's, or the refusal a Mount's own rule names |

## `FL-035` Reading a directory opened as a file raises EISDIR

| Attribute | Value |
| --- | --- |
| refines | `FL-034` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a directory |
| When | the script opens the directory to read, then reads it |
| Then | the read raises `Errno::EISDIR` |

## `FL-036` Setting a file's position moves where the next read starts

| Attribute | Value |
| --- | --- |
| relates | `FL-021` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file of `hello` |
| When | the script sets the file's position to 2, then reads the rest |
| Then | the read answers `llo` |

## `FL-037` A seek before the start or by an unknown whence raises EINVAL

| Attribute | Value |
| --- | --- |
| refines | `FL-036` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file |
| When | the script sets the position to -1, seeks to -1, and seeks by whence 9 |
| Then | each raises `Errno::EINVAL` |
