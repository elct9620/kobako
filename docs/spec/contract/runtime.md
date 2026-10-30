# Engine seams

What a third party implements to put its own wasm engine behind a Frontend. It
also covers the values that engine hands back for one invocation. Both frontends drive the
Guest through these, so an engine answering them serves either.

## Includes

- `crates/kobako-runtime/src/dispatch.rs`
- `crates/kobako-runtime/src/error.rs`
- `crates/kobako-runtime/src/filesystem.rs`
- `crates/kobako-runtime/src/filesystem/bind.rs`
- `crates/kobako-runtime/src/filesystem/overlay.rs`
- `crates/kobako-runtime/src/filesystem/tmpfs.rs`
- `crates/kobako-runtime/src/grant.rs`
- `crates/kobako-runtime/src/mount.rs`
- `crates/kobako-runtime/src/profile.rs`
- `crates/kobako-runtime/src/runtime.rs`
- `crates/kobako-runtime/src/snapshot.rs`
- `crates/kobako-runtime/src/vfs.rs`
- `crates/kobako-runtime/src/vfs/dentry.rs`
- `crates/kobako-runtime/src/vfs/filesystem.rs`
- `crates/kobako-runtime/src/vfs/inode.rs`
- `crates/kobako-runtime/src/vfs/namei.rs`
- `crates/kobako-runtime/src/vfs/openat2.rs`
- `crates/kobako-runtime/src/vfs/path.rs`
- `crates/kobako-runtime/src/vfs/rwsem.rs`
- `crates/kobako-runtime/src/vfs/stat.rs`
- `crates/kobako-runtime/src/vfs/utimes.rs`
- `crates/kobako-runtime/src/yielder.rs`

## `Runtime`

A wasm engine that runs one invocation of a Guest Binary and declares the
isolation it provides.

```rust
pub trait Runtime {}
```

## `Runtime::invoke`

Run one invocation through the given entry, routing each Call to the handler.
The Grants are this invocation's, and the engine keeps none of them afterwards.

```rust
pub trait Runtime {
    fn invoke(&self, entry: Entry<'_>, preamble: &Preamble, handler: Option<Arc<dyn DispatchHandler>>, grants: &[Grant]) -> Result<Snapshot, InvokeError>;
}
```

## `Runtime::supports`

Whether the engine presents a kind of Grant to the Guest. A Frontend refuses a
Grant of a kind it does not support. The default supports none.

```rust
pub trait Runtime {
    fn supports(&self, _kind: GrantKind) -> bool;
}
```

## `Runtime::profile`

The isolation the engine provides, which a Frontend weighs against the floor
its Host App asked for.

```rust
pub trait Runtime {
    fn profile(&self) -> Profile;
}
```

## `Runtime::compile`

The bytecode of the Preamble an invocation sends, compiled on an instance no
script runs on. An answer holds one bytecode per source Snippet, in order. Put
in their place, it still fits that Preamble; any other answer is `None`. The
default compiles nothing, and the Frontend keeps the source.

```rust
pub trait Runtime {
    fn compile(&self, _preamble: &Preamble) -> Option<Vec<Vec<u8>>>;
}
```

## `DispatchHandler`

The Frontend's side of a Call: the engine hands it each Call and relays the
Reply back to the Guest.

```rust
pub trait DispatchHandler {}
```

## `DispatchHandler::dispatch`

Answer one Call, with a Yielder for the block that rode along.

```rust
pub trait DispatchHandler {
    fn dispatch(&self, call: Call<'_>, yielder: &mut dyn Yielder) -> Option<Reply>;
}
```

## `DispatchHandler::abandoned`

Whether the Host App gave up on the invocation. The engine then ends it at the
next host call it answers. The default never gives up.

```rust
pub trait DispatchHandler {
    fn abandoned(&self) -> bool;
}
```

## `Yielder`

The engine's way back into the Guest while a Call is out. The host uses it to
run the block that rode along.

```rust
pub trait Yielder {}
```

## `Yielder::frame`

The open Call this Yielder was lent for, counted from the outermost.

```rust
pub trait Yielder {
    fn frame(&self) -> u32;
}
```

## `Yielder::yield_to_block`

Run the block of the open Call `frame` names once with the given arguments, and return its decoded Reply.

```rust
pub trait Yielder {
    fn yield_to_block(&mut self, frame: u32, args: &[u8]) -> Result<YieldReply, Trap>;
}
```

## `Entry`

Which invocation the downcall starts, by the tag it carries.

```rust
pub enum Entry {}
```

## `Snapshot`

What one invocation left behind, whether it ended in an Outcome or a Trap.

```rust
pub struct Snapshot {}
```

## `Snapshot::new`

What an engine hands back from one invocation: how it ended, both captures, and
its usage.

```rust
impl Snapshot {
    pub fn new(completion: Completion, stdout: Capture, stderr: Capture, usage: Usage) -> Self {}
}
```

## `Completion`

How an invocation ended: the Guest's final message, or the engine stopping it.

```rust
pub enum Completion {}
```

## `Capture`

One captured output channel and whether it reached its cap.

```rust
pub struct Capture {}
```

## `Usage`

What one invocation consumed of its time and memory.

```rust
pub struct Usage {}
```

## `Profile`

The isolation posture an engine declares, ordered so a floor can be compared
against it.

```rust
pub enum Profile {}
```

## `Trap`

Why the engine stopped the Guest.

```rust
pub enum Trap {}
```

## `SetupError`

Why an invocation could not start before the engine ran.

```rust
pub enum SetupError {}
```

## `InvokeError`

Why an invocation produced no Snapshot: a Trap, or a setup that failed first.

```rust
pub enum InvokeError {}
```

## `Grant`

A host resource a Frontend hands the engine for one invocation. Each kind
decides its own access.

```rust
pub enum Grant {}
```

## `GrantKind`

The kinds of Grant an engine may support.

```rust
pub enum GrantKind {}
```

## `Mount`

The Grant placing a Filesystem at a guest path, deciding its own access.

```rust
pub struct Mount {}
```

## `Mount::new`

Place a Filesystem instance at a guest path with one access, refusing a path
no Guest can name.

```rust
impl Mount {
    pub fn new(
        at: impl Into<String>,
        filesystem: Arc<Filesystem>,
        access: Access,
    ) -> Result<Self, MountPathError> {}
}
```

## `MountPathError`

A path no Mount can stand at, being no absolute guest path.

```rust
pub struct MountPathError {}
```

## `Mount::with_policy`

Let a Policy decide each open and each change in the access's place.

```rust
impl Mount {
    pub fn with_policy(self, policy: Arc<dyn Policy>) -> Self {}
}
```

## `Mount::at`

The absolute guest path the Filesystem appears at.

```rust
impl Mount {
    pub fn at(&self) -> &str {}
}
```

## `Mount::filesystem`

The Filesystem instance the Mount places.

```rust
impl Mount {
    pub fn filesystem(&self) -> &Arc<Filesystem> {}
}
```

## `Filesystem`

One Filesystem instance. Its Mounts share the directories a Guest reaches
beneath it, as bind mounts share a super block.

```rust
pub struct Filesystem {}
```

## `Filesystem::new`

The instance a Filesystem's root answers for.

```rust
impl Filesystem {
    pub fn new(root: Arc<dyn InodeOperations>) -> Arc<Self> {}
}
```

## `Filesystem::with_blocking`

The instance a Filesystem's root answers for, each call waiting for a
directory's lock through the given Blocking.

```rust
impl Filesystem {
    pub fn with_blocking(root: Arc<dyn InodeOperations>, blocking: Arc<dyn Blocking>) -> Arc<Self> {}
}
```

## `Filesystem::root`

The Filesystem's root directory.

```rust
impl Filesystem {
    pub fn root(&self) -> &Arc<dyn InodeOperations> {}
}
```

## `Dentry`

One name of a Filesystem, made by the VFS alone. A Filesystem reads the name
a call is about from it.

```rust
pub struct Dentry {}
```

## `Dentry::name`

The name in the directory above, empty for the root.

```rust
impl Dentry {
    pub fn name(&self) -> String {}
}
```

## `Dentry::path_raw`

Where the name stands now beneath the Filesystem's root, empty for the root.

```rust
impl Dentry {
    pub fn path_raw(&self) -> String {}
}
```

## `Dentry::d_add`

Record what a lookup found at the name, none where nothing stands. A Dentry
already naming something keeps it.

```rust
impl Dentry {
    pub fn d_add(&self, inode: Option<Inode>) {}
}
```

## `Dentry::d_inode`

What the name stands for, none while nothing was recorded.

```rust
impl Dentry {
    pub fn d_inode(&self) -> Option<Arc<Inode>> {}
}
```

## `Dentry::d_fsdata`

What the Filesystem keeps on the name, set once.

```rust
impl Dentry {
    pub fn d_fsdata(&self) -> &OnceLock<Arc<dyn Any + Send + Sync>> {}
}
```

## `Identity`

The device and inode numbers a Filesystem names an entry by.

```rust
pub struct Identity {}
```

## `Identity::of`

The identity some metadata names, none where its inode number of zero reports
none.

```rust
impl Identity {
    pub fn of(metadata: &Metadata) -> Option<Self> {}
}
```

## `Inode`

One entry of a Filesystem, as a lookup records it on a Dentry.

```rust
pub struct Inode {}
```

## `Inode::new`

The entry some metadata reports, answered for by its operations.

```rust
impl Inode {
    pub fn new(metadata: &Metadata, ops: Arc<dyn InodeOperations>) -> Self {}
}
```

## `Inode::by_path`

An entry its operations answer for by the path each call names.

```rust
impl Inode {
    pub fn by_path(ops: Arc<dyn InodeOperations>) -> Self {}
}
```

## `Inode::with_ops`

This entry answered for by other operations, as a wrapper wraps what it finds.

```rust
impl Inode {
    pub fn with_ops(&self, ops: Arc<dyn InodeOperations>) -> Self {}
}
```

## `Inode::ops`

What answers for the entry.

```rust
impl Inode {
    pub fn ops(&self) -> &Arc<dyn InodeOperations> {}
}
```

## `Inode::answers_by_path`

Whether its Filesystem answers for the entry by the path each call names.

```rust
impl Inode {
    pub fn answers_by_path(&self) -> bool {}
}
```

## `Inode::kind`

What kind of entry it is, none for one answered by path.

```rust
impl Inode {
    pub fn kind(&self) -> Option<FileType> {}
}
```

## `Inode::identity`

The entry's Identity, none where its Filesystem names no inode number.

```rust
impl Inode {
    pub fn identity(&self) -> Option<Identity> {}
}
```

## `Mount::allows_writes`

Whether a change beneath the Mount may be allowed at all, without asking
its Policy.

```rust
impl Mount {
    pub fn allows_writes(&self) -> bool {}
}
```

## `Denial`

Why a Mount refused a call: a change beneath a read-only Mount, or its Policy.

```rust
pub enum Denial {}
```

## `Path`

A directory beneath a Mount, as a directory descriptor holds one.

```rust
pub struct Path {}
```

## `Path::root`

The root of a Mount for an invocation running under a Profile, where each
of its descriptors starts.

```rust
impl Path {
    pub fn root(mount: Mount, profile: Profile) -> Self {}
}
```

## `Path::mount`

The Mount the directory stands beneath.

```rust
impl Path {
    pub fn mount(&self) -> &Arc<Mount> {}
}
```

## `Path::metadata`

What the directory reports of itself.

```rust
impl Path {
    pub fn metadata(&self) -> Result<Metadata, Error> {}
}
```

## `Path::entries`

The entries of the directory, without `.` or `..`.

```rust
impl Path {
    pub fn entries(&self) -> Result<Vec<DirEntry>, Error> {}
}
```

## `OpenHow`

How to open a path, as `openat2` takes its flags.

```rust
pub struct OpenHow {}
```

## `FileDescription`

What an open reached: a directory or a file.

```rust
pub enum FileDescription {}
```

## `do_file_open`

Open a path beneath a directory, as Linux's `do_file_open` serves `openat`.
Where the caller has no descriptor left, it answers too many open files.

```rust
pub fn do_file_open(start: &Path, path: &str, how: &OpenHow, room: bool) -> Result<FileDescription, Error> {}
```

## `filename_mkdirat`

Make a directory at a path beneath a directory, as Linux's `filename_mkdirat`
serves `mkdirat`.

```rust
pub fn filename_mkdirat(start: &Path, path: &str) -> Result<(), Error> {}
```

## `filename_rmdir`

Remove the empty directory at a path beneath a directory.

```rust
pub fn filename_rmdir(start: &Path, path: &str) -> Result<(), Error> {}
```

## `filename_unlinkat`

Remove the file at a path beneath a directory.

```rust
pub fn filename_unlinkat(start: &Path, path: &str) -> Result<(), Error> {}
```

## `filename_renameat2`

Move an entry within one Mount.

```rust
pub fn filename_renameat2(from: &Path, from_path: &str, to: &Path, to_path: &str) -> Result<(), Error> {}
```

## `vfs_fstatat`

What a path names, a link at its end followed where asked and reported as
itself otherwise.

```rust
pub fn vfs_fstatat(start: &Path, path: &str, follow: bool) -> Result<Metadata, Error> {}
```

## `do_readlinkat`

Refuse reading a link as unsupported once the path is known to stand.

```rust
pub fn do_readlinkat(start: &Path, path: &str) -> Result<(), Error> {}
```

## `do_utimes`

Refuse setting timestamps as unsupported once the path is known to stand.

```rust
pub fn do_utimes(start: &Path, path: &str) -> Result<(), Error> {}
```

## `PATH_MAX`

The longest path, its terminating NUL counted, as Linux's.

```rust
pub const PATH_MAX: usize = 4096;
```

## `NAME_MAX`

The longest name within a path, as Linux's.

```rust
pub const NAME_MAX: usize = 255;
```

## `Access`

What the Guest may do beneath a Mount: read, or read and write.

```rust
pub enum Access {}
```

## `Policy`

The Host App's decision for each path the Guest opens or changes, made in a
Mount's access's place.

```rust
pub trait Policy {}
```

## `Policy::allows`

Whether the Guest may reach the absolute guest path with the given access.

```rust
pub trait Policy {
    fn allows(&self, path: &str, access: Access) -> bool;
}
```

## `InodeOperations`

What one entry of a Filesystem answers, as Linux's `inode_operations`. Only
`getattr` is required. An operation an entry lacks answers none, and the
runtime answers for it as Linux's VFS does. A wrapper wraps each entry its
`lookup` finds.

```rust
pub trait InodeOperations {}
```

## `InodeOperations::getattr`

What this entry reports of itself. A link reports itself.

```rust
pub trait InodeOperations {
    fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError>;
}
```

## `InodeOperations::lookup`

What stands at the name a Dentry carries in this directory. An entry without
it is no directory to walk through.

```rust
pub trait InodeOperations {
    fn lookup(&self, _dentry: &Dentry) -> Option<Result<Lookup, FilesystemError>>;
}
```

## `InodeOperations::d_revalidate`

Whether a Dentry the runtime still holds names what it recorded. A false answer
has the runtime drop the Dentry and take the name afresh. A Filesystem without
it keeps every name valid.

```rust
pub trait InodeOperations {
    fn d_revalidate(&self, _dentry: &Dentry) -> Option<Result<bool, FilesystemError>>;
}
```

## `InodeOperations::get_link`

Where this link points, for the runtime to follow. An entry without it answers
invalid input.

```rust
pub trait InodeOperations {
    fn get_link(&self, _dentry: &Dentry) -> Option<Result<String, FilesystemError>>;
}
```

## `InodeOperations::open`

Open this file, truncating it where the options say to. The runtime refuses a
directory and a link before it asks.

```rust
pub trait InodeOperations {
    fn open(&self, _dentry: &Dentry, _options: &OpenOptions) -> Option<Result<Box<dyn File>, FilesystemError>>;
}
```

## `InodeOperations::create`

Make the file a Dentry names in this directory and open it. A directory without
it answers permission denied.

```rust
pub trait InodeOperations {
    fn create(&self, _dentry: &Dentry, _options: &OpenOptions) -> Option<Result<Box<dyn File>, FilesystemError>>;
}
```

## `InodeOperations::entries`

The entries of this directory. An entry without it answers not a directory.

```rust
pub trait InodeOperations {
    fn entries(&self, _dentry: &Dentry) -> Option<Result<Vec<DirEntry>, FilesystemError>>;
}
```

## `InodeOperations::mkdir`

Make the directory a Dentry names in this one. A directory without it answers
not permitted.

```rust
pub trait InodeOperations {
    fn mkdir(&self, _dentry: &Dentry) -> Option<Result<(), FilesystemError>>;
}
```

## `InodeOperations::rmdir`

Remove the empty directory a Dentry names in this one. A directory without it
answers not permitted.

```rust
pub trait InodeOperations {
    fn rmdir(&self, _dentry: &Dentry) -> Option<Result<(), FilesystemError>>;
}
```

## `InodeOperations::unlink`

Remove the file a Dentry names in this directory. A directory without it
answers not permitted.

```rust
pub trait InodeOperations {
    fn unlink(&self, _dentry: &Dentry) -> Option<Result<(), FilesystemError>>;
}
```

## `InodeOperations::rename`

Move what one Dentry names to another in a target directory, refusing what
rename(2) refuses at the target. The runtime has already checked the source
and never hands a move onto its own name. The target is a directory of the
same Mount, so one of another type crosses devices. A directory without it
answers not permitted.

```rust
pub trait InodeOperations {
    fn rename(&self, _from: &Dentry, _to_dir: &dyn InodeOperations, _to: &Dentry) -> Option<Result<(), FilesystemError>>;
}
```

## `Lookup`

What one name in a directory turned out to be: an entry, nothing, or an entry
stored under another spelling.

```rust
pub enum Lookup {}
```

## `File`

One open file. The engine keeps its position, so each read and write names its
offset.

```rust
pub trait File {}
```

## `File::read_at`

Read from an offset into the buffer, answering how much was read.

```rust
pub trait File {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError>;
}
```

## `File::write_at`

Write the buffer at an offset, answering how much was written.

```rust
pub trait File {
    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError>;
}
```

## `File::append`

Write the buffer at the file's end as it stands when the write lands. It
answers the offset it landed at and how much was written. A file that cannot
take both steps as one answers `Unsupported`.

```rust
pub trait File {
    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError>;
}
```

## `File::metadata`

What the open file is, and its length.

```rust
pub trait File {
    fn metadata(&self) -> Result<Metadata, FilesystemError>;
}
```

## `File::set_len`

Truncate or extend the file, extending with zeros.

```rust
pub trait File {
    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError>;
}
```

## `File::sync`

Make what was written durable. The default has nothing to flush.

```rust
pub trait File {
    fn sync(&mut self) -> Result<(), FilesystemError>;
}
```

## `File::close`

Release the file once the guest closes it, answering whether what it held was
kept. The default has nothing to release.

```rust
pub trait File {
    fn close(self: Box<Self>) -> Result<(), FilesystemError>;
}
```

## `OpenOptions`

How a file is opened: for reading, writing, creating, or truncating.

```rust
pub struct OpenOptions {}
```

## `FileType`

What a path names: a file, a directory, a link, or another kind a host holds, such as a FIFO.

```rust
pub enum FileType {}
```

## `Metadata`

What a path names, its length, and its identity.

```rust
pub struct Metadata {}
```

## `Metadata::new`

What a path names and its length, as a Filesystem reports it.

```rust
impl Metadata {
    pub fn new(file_type: FileType, len: u64) -> Self {}
}
```

## `Metadata::with_identity`

The same metadata naming the device and inode numbers a Filesystem gives what
the path holds.

```rust
impl Metadata {
    pub fn with_identity(self, dev: u64, ino: u64) -> Self {}
}
```

## `Metadata::with_nlink`

The same metadata counting the names a Filesystem gives the entry.

```rust
impl Metadata {
    pub fn with_nlink(self, nlink: u64) -> Self {}
}
```

## `Metadata::with_times`

The same metadata recording when the entry was last read, written, and
changed.

```rust
impl Metadata {
    pub fn with_times(self, accessed: Option<SystemTime>, modified: Option<SystemTime>, changed: Option<SystemTime>) -> Self {}
}
```

## `Metadata::unique_device`

A device number for a Filesystem with none of its own. No earlier call
returned it, and no host reports it.

```rust
impl Metadata {
    pub fn unique_device() -> u64 {}
}
```

## `DirEntry`

One entry of a directory: its name and what it names.

```rust
pub struct DirEntry {}
```

## `DirEntry::new`

One entry of a directory, as a Filesystem lists it.

```rust
impl DirEntry {
    pub fn new(name: impl Into<String>, file_type: FileType) -> Self {}
}
```

## `FilesystemError`

Why a Filesystem call failed. The engine hands the Guest the matching error
code.

```rust
pub enum FilesystemError {}
```

## `FilesystemError::posix_name`

The POSIX error name a failure stands for. Each host frontend reads its own
error code from this one name.

```rust
impl FilesystemError {
    pub fn posix_name(&self) -> Option<&'static str> {}
}
```

## `FilesystemError::from_posix_name`

The failure a POSIX error name stands for, so a host error maps back by name.

```rust
impl FilesystemError {
    pub fn from_posix_name(name: &str) -> Option<Self> {}
}
```

## `Tmpfs`

The Filesystem kobako ships that holds its files in memory for as long as the
object lives. Every invocation shares it.

```rust
pub struct Tmpfs {}
```

## `Tmpfs::new`

An empty Tmpfs. One seeded with files is collected from `(path, contents)`
pairs instead.

```rust
impl Tmpfs {
    pub fn new() -> Self {}
}
```

## `Tmpfs::DEFAULT_SIZE`

The most bytes a Tmpfs's files hold together unless it is given a size of
its own.

```rust
impl Tmpfs {
    pub const DEFAULT_SIZE: usize = 64 * 1024 * 1024;
}
```

## `Tmpfs::with_size`

This Tmpfs with its files holding at most the given bytes together, or
without bound for none. A write past it is refused as out of space.

```rust
impl Tmpfs {
    pub fn with_size(self, size: Option<usize>) -> Self {}
}
```

## `Tmpfs::with_nr_inodes`

This Tmpfs holding at most the given files and directories, its root
among them, or without bound for none. Without it, one is held for each
4 KiB of the size, and the root. An entry past it is refused as out of
space.

```rust
impl Tmpfs {
    pub fn with_nr_inodes(self, nr_inodes: Option<usize>) -> Self {}
}
```

## `Tmpfs::from_iter`

A Tmpfs seeded with files, each a path and its contents, its directories made
along the way. It panics on a list `Tmpfs::try_from_iter` refuses.

```rust
impl<P: AsRef<str>, C: Into<Vec<u8>>> FromIterator<(P, C)> for Tmpfs {
    fn from_iter<I: IntoIterator<Item = (P, C)>>(files: I) -> Self {}
}
```

## `Tmpfs::try_from_iter`

A Tmpfs seeded with files as `Tmpfs::from_iter` seeds them. A list no Tmpfs
holds as given answers the `SeedError` naming why.

```rust
impl Tmpfs {
    pub fn try_from_iter<P, C, I>(files: I) -> Result<Self, SeedError>
    where
        P: AsRef<str>,
        C: Into<Vec<u8>>,
        I: IntoIterator<Item = (P, C)>,
    {}
}
```

## `SeedError`

Why a list of files cannot seed a Tmpfs. The list names the root or gives a
file twice. Or it gives a file where another path needs a directory.

```rust
pub enum SeedError {}
```

## `Bind`

The Filesystem kobako ships over a host directory. Nothing outside that
directory is reachable through it.

```rust
pub struct Bind {}
```

## `Bind::new`

A Bind over the host directory at a path, refusing one that cannot be
opened.

```rust
impl Bind {
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {}
}
```

## `Overlay`

The Filesystem kobako ships that stacks read-only lower Layers under an
optional upper Tmpfs into one view.

```rust
pub struct Overlay {}
```

## `Overlay::new`

An Overlay of the given lower Layers, topmost first, under the given upper.
It is refused unless the upper is a Tmpfs and the Layers make a view.

```rust
impl Overlay {
    pub fn new(
        lower: Vec<Arc<Filesystem>>,
        upper: Option<Arc<Filesystem>>,
    ) -> Result<Self, FilesystemError> {}
}
```

## `Overlay::with_blocking`

This Overlay waiting for a copy up's lock through the given Blocking.

```rust
impl Overlay {
    pub fn with_blocking(self, blocking: Arc<dyn Blocking>) -> Self {}
}
```

## `Blocking`

How a thread waits for a lock another thread holds: a directory's, or an
Overlay's. A frontend whose threads share a lock of their own gives it up
for the wait.

```rust
pub trait Blocking: Send + Sync {}
```

## `Blocking::wait`

Run the given wait, which returns once the lock is taken.

```rust
pub trait Blocking {
    fn wait(&self, wait: &mut dyn FnMut());
}
```
