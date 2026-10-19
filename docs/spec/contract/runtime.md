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

## `Yielder`

The engine's way back into the Guest while a Call is out. The host uses it to
run the block that rode along.

```rust
pub trait Yielder {}
```

## `Yielder::yield_to_block`

Run the block once with the given arguments and return its decoded Reply.

```rust
pub trait Yielder {
    fn yield_to_block(&mut self, args: &[u8]) -> Result<YieldReply, Trap>;
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

Place a Filesystem at a guest path with one access, refusing a path that is
not an absolute guest path.

```rust
impl Mount {
    pub fn new(
        at: impl Into<String>,
        filesystem: Arc<dyn Filesystem>,
        access: Access,
    ) -> Result<Self, InvalidMountPath> {}
}
```

## `InvalidMountPath`

A path no Mount can stand at, being no absolute guest path.

```rust
pub struct InvalidMountPath {}
```

## `Mount::with_policy`

Let a Policy decide each open and each change in the access's place.

```rust
impl Mount {
    pub fn with_policy(self, policy: Arc<dyn Policy>) -> Self {}
}
```

## `Mount::guest_path`

The absolute guest path a path relative to the Mount's root stands for.

```rust
impl Mount {
    pub fn guest_path(&self, path: &str) -> String {}
}
```

## `Mount::check`

Whether the Guest may reach a path beneath the Mount with the given access.

```rust
impl Mount {
    pub fn check(&self, path: &str, access: Access) -> Result<(), Denial> {}
}
```

## `Mount::moves_directories`

Whether a directory may move beneath the Mount; not under a Policy.

```rust
impl Mount {
    pub fn moves_directories(&self) -> bool {}
}
```

## `Mount::for_invocation`

The Mount as a new invocation starts from it, over that invocation's Filesystem.

```rust
impl Mount {
    pub fn for_invocation(&self) -> Self {}
}
```

## `Denial`

Why a Mount refused a call: a change beneath a read-only Mount, or its Policy.

```rust
pub enum Denial {}
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

The files and directories behind a Mount. One may wrap another and
answer by delegating.

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

## `Filesystem::metadata`

What a path names, and its length.

```rust
pub trait Filesystem {
    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError>;
}
```

## `Filesystem::symlink_metadata`

What a path names without following a link standing there. A Filesystem
holding no links answers as `metadata` does.

```rust
pub trait Filesystem {
    fn symlink_metadata(&self, path: &str) -> Result<Metadata, FilesystemError>;
}
```

## `Filesystem::read_dir`

The entries of a directory.

```rust
pub trait Filesystem {
    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError>;
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

Move a file or directory to another path, refusing what the given Rename does
not allow. What stands at the source is checked as it moves.

```rust
pub trait Filesystem {
    fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError>;
}
```

## `Filesystem::for_invocation`

The Filesystem a new invocation starts from in this one's place. The default
shares this one with every invocation.

```rust
pub trait Filesystem {
    fn for_invocation(&self) -> Option<Arc<dyn Filesystem>>;
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
answers the offset it landed at and how much was written. The default reads
the length and writes there.

```rust
pub trait File {
    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {}
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

## `Rename::allows`

Whether an entry of the given kind may move, or the refusal of it.

```rust
impl Rename {
    pub fn allows(self, kind: Kind) -> Result<(), FilesystemError> {}
}
```

## `Kind`

What a path names: a file, a directory, or a link.

```rust
pub enum Kind {}
```

## `Metadata`

What a path names, and its length.

```rust
pub struct Metadata {}
```

## `DirEntry`

One entry of a directory: its name and what it names.

```rust
pub struct DirEntry {}
```

## `FilesystemError`

Why a Filesystem call failed. The engine hands the Guest the matching error
code.

```rust
pub enum FilesystemError {}
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

## `Temporary`

The Filesystem kobako ships that holds its files in memory and starts every
invocation empty.

```rust
pub struct Temporary {}
```

## `Temporary::new`

An empty Temporary.

```rust
impl Temporary {
    pub fn new() -> Self {}
}
```

## `Temporary::with_limit`

This Temporary with its files holding at most the given bytes together, or
without bound for none. Every Temporary handed out in its place keeps it.

```rust
impl Temporary {
    pub fn with_limit(self, limit: Option<usize>) -> Self {}
}
```

## `Temporary::with_inodes`

This Temporary holding at most the given files and directories, as a
Memory given them does. Every Temporary handed out in its place keeps it.

```rust
impl Temporary {
    pub fn with_inodes(self, inodes: Option<usize>) -> Self {}
}
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
