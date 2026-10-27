# Engine seams

What a third party implements to put its own wasm engine behind a Frontend. It
also covers the values that engine hands back for one invocation. Both frontends drive the
Guest through these, so an engine answering them serves either.

## Includes

- `crates/kobako-runtime/src/**/*.rs`

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
        super_block: Arc<SuperBlock>,
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

## `Mount::super_block`

The Filesystem instance the Mount places.

```rust
impl Mount {
    pub fn super_block(&self) -> &Arc<SuperBlock> {}
}
```

## `SuperBlock`

One Filesystem instance. Its Mounts share the directories a Guest reaches
beneath it, as bind mounts share a super block.

```rust
pub struct SuperBlock {}
```

## `SuperBlock::new`

The instance a Filesystem's root answers for.

```rust
impl SuperBlock {
    pub fn new(root: Arc<dyn Filesystem>) -> Arc<Self> {}
}
```

## `SuperBlock::root`

The Filesystem's root directory.

```rust
impl SuperBlock {
    pub fn root(&self) -> &Arc<dyn Filesystem> {}
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

## `Opened`

What an open reached: a directory or a file.

```rust
pub enum Opened {}
```

## `open`

Open a path beneath a directory, answering too many open files where the
caller has no descriptor left.

```rust
pub fn open(start: &Path, path: &str, how: &OpenHow, room: bool) -> Result<Opened, Error> {}
```

## `create_dir`

Make a directory at a path beneath a directory.

```rust
pub fn create_dir(start: &Path, path: &str) -> Result<(), Error> {}
```

## `remove_dir`

Remove the empty directory at a path beneath a directory.

```rust
pub fn remove_dir(start: &Path, path: &str) -> Result<(), Error> {}
```

## `remove_file`

Remove the file at a path beneath a directory.

```rust
pub fn remove_file(start: &Path, path: &str) -> Result<(), Error> {}
```

## `rename`

Move an entry within one Mount.

```rust
pub fn rename(from: &Path, from_path: &str, to: &Path, to_path: &str) -> Result<(), Error> {}
```

## `metadata`

What a path names, a link at its end followed.

```rust
pub fn metadata(start: &Path, path: &str) -> Result<Metadata, Error> {}
```

## `symlink_metadata`

What a path names, a link at its end reported as itself.

```rust
pub fn symlink_metadata(start: &Path, path: &str) -> Result<Metadata, Error> {}
```

## `read_link`

Refuse reading a link as unsupported once the path is known to stand.

```rust
pub fn read_link(start: &Path, path: &str) -> Result<(), Error> {}
```

## `set_times`

Refuse setting timestamps as unsupported once the path is known to stand.

```rust
pub fn set_times(start: &Path, path: &str) -> Result<(), Error> {}
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

What the Guest may do beneath a Mount, or an Overlay in a Layer: read, or
read and write.

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

## `Filesystem`

The files and directories beneath one directory, as the `*at` calls reach
them. That directory is a Mount's root, or one opened beneath it. One may wrap another
and answer by delegating, wrapping each directory it opens too. The methods
keeping a guarantee have no default, so a wrapper forwards them.

```rust
pub trait Filesystem {}
```

## `Filesystem::open`

Open the file at a path, creating or truncating it where the options say to.

```rust
pub trait Filesystem {
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError>;
}
```

## `Filesystem::open_dir_nofollow`

The directory of a name in this one, as a Filesystem of its own. Anything
else standing there is refused, a link among them.

```rust
pub trait Filesystem {
    fn open_dir_nofollow(&self, name: &str) -> Result<Arc<dyn Filesystem>, FilesystemError>;
}
```

## `Filesystem::metadata`

What the directory reports of itself, by default what `symlink_metadata`
answers for the empty path.

```rust
pub trait Filesystem {
    fn metadata(&self) -> Result<Metadata, FilesystemError> {}
}
```

## `Filesystem::symlink_metadata`

What a path names without following a link standing there.

```rust
pub trait Filesystem {
    fn symlink_metadata(&self, path: &str) -> Result<Metadata, FilesystemError>;
}
```

## `Filesystem::stored_name`

A name in this directory spelled as it is stored. A name it does not hold
stays as given. A Filesystem matching names exactly answers the name itself.

```rust
pub trait Filesystem {
    fn stored_name(&self, name: &str) -> Result<String, FilesystemError>;
}
```

## `Filesystem::read_link`

Where a link in this directory points, for the runtime to follow. Anything
else standing there is invalid input, as it is for a Filesystem holding no
links.

```rust
pub trait Filesystem {
    fn read_link(&self, name: &str) -> Result<String, FilesystemError>;
}
```

## `Filesystem::entries`

The entries of this directory.

```rust
pub trait Filesystem {
    fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError>;
}
```

## `Filesystem::create_dir`

Create a directory.

```rust
pub trait Filesystem {
    fn create_dir(&self, path: &str) -> Result<(), FilesystemError>;
}
```

## `Filesystem::remove_dir`

Remove an empty directory.

```rust
pub trait Filesystem {
    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError>;
}
```

## `Filesystem::remove_file`

Remove a file.

```rust
pub trait Filesystem {
    fn remove_file(&self, path: &str) -> Result<(), FilesystemError>;
}
```

## `Filesystem::rename`

Move a file or directory to a path beneath a target directory, refusing what
the given Rename does not allow. The target is a directory of the same Mount,
so one of another type crosses devices. What stands at the source is checked
as it moves.

```rust
pub trait Filesystem {
    fn rename(&self, from: &str, to_dir: &dyn Filesystem, to: &str, rename: Rename) -> Result<(), FilesystemError>;
}
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

## `Rename`

What a rename may move: whatever stands at the source, only a file, or only
a directory.

```rust
pub enum Rename {}
```

## `Rename::check`

Refuse moving an entry of a file type the rename may not move.

```rust
impl Rename {
    pub fn check(self, file_type: FileType) -> Result<(), FilesystemError> {}
}
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

## `Memory`

The Filesystem kobako ships that holds its files in memory for as long as the
object lives. Every invocation shares it.

```rust
pub struct Memory {}
```

## `Memory::new`

An empty Memory. One seeded with files is collected from `(path, contents)`
pairs instead.

```rust
impl Memory {
    pub fn new() -> Self {}
}
```

## `Memory::DEFAULT_LIMIT`

The most bytes a Memory's files hold together unless it is given a limit of
its own.

```rust
impl Memory {
    pub const DEFAULT_LIMIT: usize = 64 * 1024 * 1024;
}
```

## `Memory::with_limit`

This Memory with its files holding at most the given bytes together, or
without bound for none. A write past it is refused as out of space.

```rust
impl Memory {
    pub fn with_limit(self, limit: Option<usize>) -> Self {}
}
```

## `Memory::with_inodes`

This Memory holding at most the given files and directories, its root
among them, or without bound for none. Without it, one is held for each
4 KiB of the limit, and the root. An entry past it is refused as out of
space.

```rust
impl Memory {
    pub fn with_inodes(self, inodes: Option<usize>) -> Self {}
}
```

## `Memory::from_iter`

A Memory seeded with files, each a path and its contents, its directories made
along the way. It panics on a list `Memory::try_from_iter` refuses.

```rust
impl<P: AsRef<str>, C: Into<Vec<u8>>> FromIterator<(P, C)> for Memory {
    fn from_iter<I: IntoIterator<Item = (P, C)>>(files: I) -> Self {}
}
```

## `Memory::try_from_iter`

A Memory seeded with files as `Memory::from_iter` seeds them. A list no Memory
holds as given answers the `SeedError` naming why.

```rust
impl Memory {
    pub fn try_from_iter<P, C, I>(files: I) -> Result<Self, SeedError>
    where
        P: AsRef<str>,
        C: Into<Vec<u8>>,
        I: IntoIterator<Item = (P, C)>,
    {}
}
```

## `SeedError`

Why a list of files cannot seed a Memory. The list names the root or gives a
file twice. Or it gives a file where another path needs a directory.

```rust
pub enum SeedError {}
```

## `Directory`

The Filesystem kobako ships over a host directory. Nothing outside that
directory is reachable through it.

```rust
pub struct Directory {}
```

## `Directory::new`

A Directory over the host directory at a path, refusing one that cannot be
opened.

```rust
impl Directory {
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {}
}
```

## `Overlay`

The Filesystem kobako ships that stacks Layers into one view, each read-only or
writable on its own.

```rust
pub struct Overlay {}
```

## `Overlay::new`

An Overlay with no Layer yet.

```rust
impl Overlay {
    pub fn new() -> Self {}
}
```

## `Overlay::layer`

This Overlay with a Filesystem as a new Layer beneath the ones already added,
allowing the given access there.

```rust
impl Overlay {
    pub fn layer(self, filesystem: Arc<dyn Filesystem>, access: Access) -> Self {}
}
```

## `Overlay::with_blocking`

This Overlay waiting for its lock through the given Blocking.

```rust
impl Overlay {
    pub fn with_blocking(self, blocking: Arc<dyn Blocking>) -> Self {}
}
```

## `Blocking`

How a thread waits for an Overlay's lock another thread holds. A frontend
whose threads share a lock of their own gives it up for the wait.

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
