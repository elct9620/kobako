# File

What a script reaches through the default Guest Binary's `File`, beneath the paths a Mount places.

```
script ─ File / IO ─> wasi-libc ─ preopen at the Mount's path ─> runtime ─> Filesystem
```

### Why these scenarios

`File` is the guest's half of a Mount. It ships in the default Guest Binary, so a script always names it. What it reaches is the Mount's to decide. With no Mount it reaches nothing, and says so as a call the platform lacks.

The failures are CRuby's classes, so a script handles them as it would on any host. A flag removes `File` outright for a Host App that wants no file surface at all.

`File::Stat` answers what preview1 hands the guest. Whatever it could only answer wrongly it leaves undefined, so a script fails instead of reading a wrong answer.

A sync, a truncate, a positioned read or write, and an advice each reach the file's Filesystem. Each answers as CRuby on Linux does, on a file and on a captured stream alike. One answer differs: preview1 counts an offset from zero, so an advice from a negative offset is refused. CRuby on Linux takes that advice and answers `nil`.

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
- `test/e2e/test_file_stat.rb`
- `test/e2e/test_file_stat_host.rb`
- `test/e2e/test_file_stat_ino.rb`
- `test/e2e/test_file_stat_refusal.rb`
- `test/e2e/test_file_sync.rb`
- `test/e2e/test_file_truncate.rb`
- `test/e2e/test_io_positioned.rb`
- `test/e2e/test_file_position.rb`

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
| Given | a script holding a file whose Filesystem refuses its close as denied, or as a file it does not hold |
| When | the script closes that file |
| Then | the close raises `Errno::EACCES` or `Errno::EBADF`, the name the Filesystem gave, and the file is closed |

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
| Then | it reads `Unknown error 9999`, as glibc's `strerror` words it, and its `errno` is 9999 |

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
| When | the script opens, reads, writes, asks about, or lists a path holding a NUL byte |
| Then | each call raises `ArgumentError` |

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

## `FL-038` A report says what a path names

| Attribute | Value |
| --- | --- |
| relates | `FL-001` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file, an empty file, and a directory |
| When | the script asks `File.stat` or `File::Stat.new` about each |
| Then | each answers its size, its kind, and its count of names, as CRuby does on Linux |

## `FL-039` A stat follows a link, and an lstat reports it

| Attribute | Value |
| --- | --- |
| refines | `FL-038` |
| relates | `RT-097` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount holding a link to a file and a link to nothing |
| When | the script asks `File.stat`, `File.lstat`, `File.ftype`, and `File.symlink?` about them |
| Then | `File.stat` reads the file; the others read each link as itself |

## `FL-040` The class calls answer without raising for a missing file

| Attribute | Value |
| --- | --- |
| refines | `FL-038` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the script asks `File.size?`, `File.zero?`, and `File.empty?` about a missing file |
| Then | `File.size?` answers `nil`, and the others `false` |

## `FL-041` A report of a missing file raises ENOENT

| Attribute | Value |
| --- | --- |
| refines | `FL-038` |
| relates | `FL-005` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount that holds no file at a path |
| When | the script asks `File.stat`, `File.lstat`, `File.ftype`, and `File::Stat.new` about it |
| Then | each raises `Errno::ENOENT` |

## `FL-042` A report with nothing mounted is unimplemented

| Attribute | Value |
| --- | --- |
| refines | `FL-038` |
| relates | `FL-004` |

| Step | Statement |
| --- | --- |
| Given | an invocation with nothing mounted |
| When | the script asks `File.stat` or any class call built on one |
| Then | each raises `NotImplementedError` |

## `FL-043` A file reports itself, and a report keeps what it read

| Attribute | Value |
| --- | --- |
| refines | `FL-038` |
| relates | `RT-183` |

| Step | Statement |
| --- | --- |
| Given | a permissive guest holding open a file beneath a Mount, with a report of it |
| When | the file grows, and the script reads the report and asks the file again |
| Then | the report keeps its size, and each report of the file reads one inode number |

## `FL-044` A file whose name is gone counts no names

| Attribute | Value |
| --- | --- |
| refines | `FL-043` |
| relates | `FS-141` |

| Step | Statement |
| --- | --- |
| Given | a guest holding open three files of a Tmpfs |
| When | it unlinks one, and renames the second onto the third |
| Then | the unlinked and the replaced count no names, and the moved one counts one |

## `FL-045` A closed file and a bad path raise what CRuby raises

| Attribute | Value |
| --- | --- |
| refines | `FL-038` |
| relates | `FL-033` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount, and a file the script closed |
| When | the script asks the file, a path holding a NUL byte, and an Integer for a report |
| Then | they raise `IOError`, `ArgumentError`, and `TypeError` |

## `FL-046` An inode number is read whole

| Attribute | Value |
| --- | --- |
| refines | `RT-183` |
| relates | `FL-020` |

| Step | Statement |
| --- | --- |
| Given | a permissive guest and many entries, whose inode numbers pass 2\*\*63 |
| When | the script reads each one's inode number |
| Then | each is the number preview1 hands a guest |

## `FL-047` What a report cannot answer truly it leaves undefined

| Attribute | Value |
| --- | --- |
| refines | `FL-038` |
| relates | `MR-010`, `FL-008` |

| Step | Statement |
| --- | --- |
| Given | a report of a file |
| When | a script asks for a time, a device, an owner, a permission, or a FIFO's kind |
| Then | no such method is defined, and neither `File.identical?` nor `IO#stat` is |

## `FL-048` A special file reads as preview1 names it

| Attribute | Value |
| --- | --- |
| refines | `FL-038` |
| relates | `RT-195` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed Bind Mounts holding a device, a socket, and a FIFO |
| When | the script asks a report of each |
| Then | the device and the socket read as themselves, and the FIFO reads `"unknown"` |

## `FL-049` A block leaves a report alone and still refuses the open

| Attribute | Value |
| --- | --- |
| refines | `FL-038` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind Mount whose block refuses a path |
| When | the script asks a report of that path, and opens it |
| Then | the report answers, and the open raises `Errno::EACCES` |

## `FL-059` A sync reaches the file's Filesystem

| Attribute | Value |
| --- | --- |
| relates | `RF-021`, `FS-140` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox mounting a Ruby Filesystem whose file records each `fsync` |
| When | the script opens that file and calls `fsync` and `fdatasync` on it |
| Then | each call answers 0, and the file records two syncs |

## `FL-060` A sync of a captured stream raises EINVAL

| Attribute | Value |
| --- | --- |
| refines | `FL-059` |
| relates | `IO-004` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with nothing mounted |
| When | the script calls `fsync` and `fdatasync` on `$stdout` |
| Then | each raises `Errno::EINVAL`, as CRuby does on a pipe |

## `FL-061` A truncate sets a file's length

| Attribute | Value |
| --- | --- |
| relates | `FL-032` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount over a file holding `hello` |
| When | the script truncates the path to 2 bytes, then truncates the open file to 4 bytes |
| Then | each call answers 0, and the file holds `he` and two zero bytes |

## `FL-062` A truncate a file or a length cannot take raises what CRuby raises

| Attribute | Value |
| --- | --- |
| refines | `FL-061` |
| relates | `FL-017` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount over a file and a directory |
| When | the script truncates a file opened to read, a negative length, the directory, and a missing path |
| Then | they raise `IOError`, `Errno::EINVAL`, `Errno::EISDIR`, and `Errno::ENOENT` |

## `FL-063` A truncate by path with nothing mounted is unimplemented

| Attribute | Value |
| --- | --- |
| refines | `FL-061` |
| relates | `FL-004` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed no Mount |
| When | the script truncates a path with `File.truncate` |
| Then | the script raises `NotImplementedError` |

## `FL-064` A positioned read and write leave the file's position alone

| Attribute | Value |
| --- | --- |
| relates | `FL-036` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file of a read-write Mount open to read and write, holding `hello` |
| When | it reads 3 bytes from offset 1 with `pread`, writes `ZZ` at offset 3 and `!` at offset 8 with `pwrite` |
| Then | `pread` answers `ell`, each `pwrite` answers its length, zero bytes fill the gap, and the position stays 0 |

## `FL-065` A positioned read at the end raises EOFError

| Attribute | Value |
| --- | --- |
| refines | `FL-064` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file of 5 bytes open to read |
| When | it reads 3 bytes and then 0 bytes from offset 10 with `pread` |
| Then | the first raises `EOFError`, and the second answers an empty String |

## `FL-066` A positioned read or write the stream cannot take raises what CRuby raises

| Attribute | Value |
| --- | --- |
| refines | `FL-064` |
| relates | `FL-017` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file open only to read, and `$stdout` |
| When | it writes to the file, reads `$stdout`, writes `$stdout`, and reads from a negative offset or length |
| Then | they raise `IOError`, `IOError`, `Errno::ESPIPE`, `Errno::EINVAL`, and `ArgumentError` |

## `FL-067` An advice answers nil and changes nothing read

| Attribute | Value |
| --- | --- |
| relates | `FL-064` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file open to read |
| When | it gives each of the six advices CRuby names with `advise`, then reads the file |
| Then | each advice answers `nil`, and the read answers what the file holds |

## `FL-068` An advice CRuby refuses is refused

| Attribute | Value |
| --- | --- |
| refines | `FL-067` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file open to read, and `$stdout` |
| When | it advises the file with a String, an unknown Symbol, and a negative length, then advises `$stdout` |
| Then | they raise `TypeError`, `NotImplementedError`, `Errno::EINVAL`, and `Errno::ESPIPE` |

## `FL-069` An advice from a negative offset raises EINVAL

| Attribute | Value |
| --- | --- |
| refines | `FL-067` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file open to read |
| When | it gives an advice from offset -1 |
| Then | the script raises `Errno::EINVAL` |

## `FL-070` A closed file refuses a sync, a truncate, a positioned transfer, and an advice

| Attribute | Value |
| --- | --- |
| relates | `FL-045` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file it opened and then closed |
| When | it calls `fsync`, `fdatasync`, `truncate`, `pread`, `pwrite`, and `advise` on it |
| Then | each raises `IOError` |

## `FL-074` A positioned read past what a file holds answers what it holds

| Attribute | Value |
| --- | --- |
| refines | `FL-064` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file of 5 bytes open to read, under the default memory limit of 4 MiB |
| When | it reads 4 MiB from offset 0 with `pread` |
| Then | `pread` answers the 5 bytes |

## `FL-075` A positioned read fills a buffer it is given

| Attribute | Value |
| --- | --- |
| refines | `FL-064` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file holding `hello` open to read, and a String holding `stale text` |
| When | it reads 2 bytes from offset 1 with `pread` into that String |
| Then | `pread` answers that same String, now holding `el` |

## `FL-076` An advice takes nil as offset or length 0

| Attribute | Value |
| --- | --- |
| refines | `FL-067` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file open to read |
| When | it gives an advice with `nil` as its offset, and another with `nil` as its length |
| Then | each answers `nil` |

## `FL-077` A directory opened as a file has no position

| Attribute | Value |
| --- | --- |
| refines | `FL-035` |

| Step | Statement |
| --- | --- |
| Given | a script holding a directory opened as a `File` |
| When | it seeks, rewinds, reads its position, or sets it |
| Then | each raises `Errno::EISDIR`. WASI keeps no position for a directory, where CRuby's answers one |

## `FL-078` A file opened only to write refuses an end check

| Attribute | Value |
| --- | --- |
| refines | `FL-045` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file opened only to write |
| When | it asks `eof?` |
| Then | it raises `IOError` "not opened for reading", as CRuby's does |

## `FL-079` A second Linux name for an errno is the same Errno class

| Attribute | Value |
| --- | --- |
| refines | `FL-016` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the script reads `Errno::EWOULDBLOCK`, `Errno::EDEADLOCK`, and `Errno::EOPNOTSUPP` |
| Then | each is `Errno::EAGAIN`, `Errno::EDEADLK`, and `Errno::ENOTSUP`, as CRuby's `set_syserr` names a number twice |

## `FL-080` Every errno the runtime answers names an Errno class

| Attribute | Value |
| --- | --- |
| refines | `FL-016` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the script reads the `errno` of `Errno::ESTALE` and of `Errno::EALREADY` |
| Then | they read 116 and 114, the numbers Linux gives them |

## `FL-081` SystemCallError takes its arguments as CRuby's does

| Attribute | Value |
| --- | --- |
| refines | `FL-016` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the script builds `SystemCallError` bare, then with 2⁴⁰ alone, and `Errno::ENOENT` with `m` and `f`, then with three |
| Then | they raise `ArgumentError`, `RangeError`, read `No such file or directory @ f - m`, and raise `ArgumentError` |

## `FL-082` A mode naming encodings opens as the mode before them

| Attribute | Value |
| --- | --- |
| refines | `FL-026` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file |
| When | the script opens it with `r:UTF-8` and with `rb:BOM\|utf-8` |
| Then | each opens to read, as CRuby's `rb_io_modestr_fmode` stops at the `:` |

## `FL-083` The path helpers read a name as CRuby's do

| Attribute | Value |
| --- | --- |
| relates | `FL-033` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the script asks `extname` of `foo.` and `..rb`, joins `a//` with `/b` and `a` with an Array. It asks `basename` of bytes not UTF-8 |
| Then | they answer `.`, nothing, `a/b`, the parts joined, and the bytes as they stand |

## `FL-084` A file reads lines as a socket does

| Attribute | Value |
| --- | --- |
| relates | `NW-095` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file of lines and a blank run |
| When | the script reads it with `gets(chomp: true)`, `gets("")`, `gets(3)`, and `readlines(chomp: true)` |
| Then | each answers as CRuby's `rb_io_getline` does, and `each_line(0)` raises `ArgumentError` |

## `FL-085` A seek takes its whence by name

| Attribute | Value |
| --- | --- |
| relates | `FL-021` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file |
| When | the script seeks four bytes before `:END` |
| Then | it stands four bytes before the end, as CRuby's `interpret_seek_whence` reads `:END` |

## `FL-086` A seek by a whence no name stands for raises TypeError

| Attribute | Value |
| --- | --- |
| refines | `FL-085` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file |
| When | the script seeks by the Symbol `:FOO` |
| Then | it raises `TypeError`, as CRuby's `NUM2INT` refuses a Symbol |

## `FL-087` A seek reads a Float whence as the Integer it truncates to

| Attribute | Value |
| --- | --- |
| refines | `FL-085` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file |
| When | the script seeks by the whence `1.5` |
| Then | it seeks from where it stands, as CRuby's `rb_num2long` truncates a Float |

## `FL-088` A number a file call takes converts through to_int

| Attribute | Value |
| --- | --- |
| refines | `FL-087` |
| relates | `IO-054` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount holding a file |
| When | the script reads a line limited by an object answering `to_int` 2, then by `true` |
| Then | the line holds two bytes, and `true` raises `TypeError` naming it, as CRuby's `NUM2LONG` |
