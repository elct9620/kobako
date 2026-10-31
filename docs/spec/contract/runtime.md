# Engine seams

What a third party implements to put its own wasm engine behind a Frontend. It
also covers the values that engine hands back for one invocation. Both frontends drive the
Guest through these, so an engine answering them serves either.

## Includes

- `crates/kobako-runtime/src/dispatch.rs`
- `crates/kobako-runtime/src/endpoint.rs`
- `crates/kobako-runtime/src/endpoint/capture.rs`
- `crates/kobako-runtime/src/endpoint/dispatch.rs`
- `crates/kobako-runtime/src/endpoint/invoke.rs`
- `crates/kobako-runtime/src/endpoint/memory_cap.rs`
- `crates/kobako-runtime/src/error.rs`
- `crates/kobako-runtime/src/filesystem.rs`
- `crates/kobako-runtime/src/filesystem/bind.rs`
- `crates/kobako-runtime/src/filesystem/overlay.rs`
- `crates/kobako-runtime/src/filesystem/tmpfs.rs`
- `crates/kobako-runtime/src/grant.rs`
- `crates/kobako-runtime/src/mount.rs`
- `crates/kobako-runtime/src/profile.rs`
- `crates/kobako-runtime/src/runtime.rs`
- `crates/kobako-runtime/src/sched.rs`
- `crates/kobako-runtime/src/security.rs`
- `crates/kobako-runtime/src/snapshot.rs`
- `crates/kobako-runtime/src/vfs.rs`
- `crates/kobako-runtime/src/vfs/attr.rs`
- `crates/kobako-runtime/src/vfs/cred.rs`
- `crates/kobako-runtime/src/vfs/dentry.rs`
- `crates/kobako-runtime/src/vfs/errno.rs`
- `crates/kobako-runtime/src/vfs/mount.rs`
- `crates/kobako-runtime/src/vfs/file.rs`
- `crates/kobako-runtime/src/vfs/super_block.rs`
- `crates/kobako-runtime/src/vfs/inode.rs`
- `crates/kobako-runtime/src/vfs/namei.rs`
- `crates/kobako-runtime/src/vfs/openat2.rs`
- `crates/kobako-runtime/src/vfs/path.rs`
- `crates/kobako-runtime/src/vfs/pipe.rs`
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

## `GuestInstance`

One guest instance as the host ABI endpoint drives it. A Driver implements it
over its Engine's memory and exports.

```rust
pub trait GuestInstance {}
```

## `GuestInstance::calls`

The state of the Calls open on this instance.

```rust
pub trait GuestInstance {
    fn calls(&mut self) -> &mut Calls<Self::Fault>;
}
```

## `GuestInstance::memory`

The guest's linear memory.

```rust
pub trait GuestInstance {
    fn memory(&mut self) -> Option<&mut [u8]>;
}
```

## `GuestInstance::alloc`

Allocate through `__kobako_alloc`, answering the address.

```rust
pub trait GuestInstance {
    fn alloc(&mut self, len: u32) -> Option<Result<u32, Self::Fault>>;
}
```

## `GuestInstance::downcall`

Call `__kobako_downcall`, answering the packed place of its answer.

```rust
pub trait GuestInstance {
    fn downcall(&mut self, ptr: i32, len: i32) -> Option<Result<u64, Self::Fault>>;
}
```

## `GuestInstance::cap_trap`

The cap a fault carries, if one of the invocation's caps fired.

```rust
pub trait GuestInstance {
    fn cap_trap(fault: &Self::Fault) -> Option<Trap>;
}
```

## `Calls`

The state of the Calls open on one instance, which a Driver keeps for it.

```rust
pub struct Calls<F> {}
```

## `Calls::bind`

Have a handler answer the Calls on this instance.

```rust
impl Calls {
    pub fn bind(&mut self, handler: Arc<dyn DispatchHandler>) {}
}
```

## `Calls::abandoned`

Whether the host gave up on the invocation, which ends at the next host call.

```rust
impl Calls {
    pub fn abandoned(&self) -> bool {}
}
```

## `UpcallEnd`

How an upcall ends an invocation instead of answering.

```rust
pub enum UpcallEnd<F> {
    Fault(F),
    Abandoned,
    Unwritable(&'static str),
}
```

## `handle`

Answer one `__kobako_upcall` call, or say what ends the invocation instead.

```rust
pub fn handle<G: GuestInstance>(guest: &mut G, req_ptr: i32, req_len: i32) -> Result<i64, UpcallEnd<G::Fault>> {}
```

## `ABANDONED`

The message of the trap ending an invocation its host gave up on.

```rust
pub const ABANDONED: &str = "the host abandoned the invocation";
```

## `RUNTIME_INCOMPATIBLE`

The message for a required guest export that is absent or mistyped.

```rust
pub const RUNTIME_INCOMPATIBLE: &str = "the Sandbox runtime is incompatible; rebuild it against the kobako version in use";
```

## `stdin_content`

What the guest reads from stdin: the Preamble behind its length prefix.

```rust
pub fn stdin_content(preamble: &[u8]) -> Result<Vec<u8>, &'static str> {}
```

## `request_of`

The downcall an entry is.

```rust
pub fn request_of(entry: Entry<'_>) -> Vec<u8> {}
```

## `write_request`

Write a request into a guest buffer, answering where.

```rust
pub fn write_request<G: GuestInstance>(guest: &mut G, envelope: &[u8]) -> Result<(i32, i32), InvokeError> {}
```

## `read_outcome`

Read the Outcome a downcall answered with.

```rust
pub fn read_outcome<G: GuestInstance>(guest: &mut G, packed: u64) -> Result<Outcome, Trap> {}
```

## `read_compile_reply`

Read the answer a compile downcall gave.

```rust
pub fn read_compile_reply<G: GuestInstance>(guest: &mut G, packed: u64) -> Result<CompileReply, Trap> {}
```

## `compiled_bytecodes`

What a compile answered to replay in place of the source snippets, where it can
be replayed.

```rust
pub fn compiled_bytecodes(preamble: &Preamble, reply: CompileReply) -> Option<Vec<Vec<u8>>> {}
```

## `pipe_capacity`

The size of a capture pipe, one byte past its cap.

```rust
pub fn pipe_capacity(cap: Option<usize>) -> usize {}
```

## `clip_capture`

A captured channel clipped back to its cap, and whether it passed it.

```rust
pub fn clip_capture(mut raw: Vec<u8>, cap: Option<usize>) -> (Vec<u8>, bool) {}
```

## `MemoryCap`

The budget one invocation's memory grows against.

```rust
pub struct MemoryCap {}
```

## `MemoryCap::new`

A budget of a cap, or of none.

```rust
impl MemoryCap {
    pub fn new(max_memory: Option<usize>) -> Self {}
}
```

## `MemoryCap::activate`

Arm the cap for one invocation from the memory it starts with.

```rust
impl MemoryCap {
    pub fn activate(&mut self, baseline: usize) {}
}
```

## `MemoryCap::deactivate`

Disarm the cap once the export returns.

```rust
impl MemoryCap {
    pub fn deactivate(&mut self) {}
}
```

## `MemoryCap::peak`

The most the invocation's memory grew.

```rust
impl MemoryCap {
    pub fn peak(&self) -> usize {}
}
```

## `MemoryCap::growing`

Charge a growth, refusing one past the cap.

```rust
impl MemoryCap {
    pub fn growing(&mut self, desired: usize) -> Result<(), MemoryLimitExceeded> {}
}
```

## `MemoryLimitExceeded`

A growth past the per-invocation memory cap.

```rust
pub struct MemoryLimitExceeded {}
```

## `MemoryLimitExceeded::new`

The growth to a size past a limit.

```rust
impl MemoryLimitExceeded {
    pub fn new(desired: usize, limit: usize) -> Self {}
}
```

## `Profile::guest_metadata`

What a Filesystem reports, as a Guest on one posture sees it, whichever ABI a
Driver encodes it into. A Driver shows only this.

```rust
impl Profile {
    pub fn guest_metadata(self, metadata: Metadata) -> Metadata {}
}
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

Let a Policy also judge each open and each change the access allows.

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

## `SuperBlock`

One Filesystem instance, as Linux's `super_block`. A Host App names it
`filesystem::Filesystem`. Its Mounts share the directories a Guest reaches
beneath it, as bind mounts share a super block.

```rust
pub struct SuperBlock {}
```

## `SuperBlock::new`

The instance a Filesystem's root answers for.

```rust
impl SuperBlock {
    pub fn new(root: Arc<dyn InodeOperations>) -> Arc<Self> {}
}
```

## `SuperBlock::root`

The Filesystem's root directory.

```rust
impl SuperBlock {
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

## `Dentry::add`

Record what a lookup found at the name, none where nothing stands. A Dentry
already naming something keeps it.

```rust
impl Dentry {
    pub fn add(&self, inode: Option<Inode>) {}
}
```

## `Dentry::inode`

What the name stands for, none while nothing was recorded.

```rust
impl Dentry {
    pub fn inode(&self) -> Option<Arc<Inode>> {}
}
```

## `Dentry::ops`

What answers about this name, the same for every Dentry of one Filesystem.

```rust
impl Dentry {
    pub fn ops(&self) -> Option<&Arc<dyn DentryOperations>> {}
}
```

## `Dentry::fsdata`

What the Filesystem keeps on the name, set once.

```rust
impl Dentry {
    pub fn fsdata(&self) -> &OnceLock<Arc<dyn Any + Send + Sync>> {}
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

## `Identity::onto`

The same metadata naming this identity, for an open file a Filesystem answers
for apart from its path.

```rust
impl Identity {
    pub fn onto(self, metadata: Metadata) -> Metadata {}
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

## `Inode::private`

What the Filesystem keeps on the entry, set once.

```rust
impl Inode {
    pub fn private(&self) -> &OnceLock<Arc<dyn Any + Send + Sync>> {}
}
```

## `Inode::kind`

What kind of entry it is.

```rust
impl Inode {
    pub fn kind(&self) -> FileType {}
}
```

## `Inode::identity`

The entry's Identity, none where its Filesystem names no inode number.

```rust
impl Inode {
    pub fn identity(&self) -> Option<Identity> {}
}
```

## `Mount::mnt`

The Filesystem as the runtime walks it.

```rust
impl Mount {
    pub fn mnt(&self) -> &Arc<VfsMount> {}
}
```

## `Denial`

Why a call was refused: a change beneath a read-only mount, or a security hook.

```rust
pub enum Denial {}
```

## `Path`

A directory beneath a Mount, as a directory descriptor holds one.

```rust
pub struct Path {}
```

## `Path::root`

The root of a mounted Filesystem, where each of its descriptors starts.

```rust
impl Path {
    pub fn root(mnt: &Arc<VfsMount>) -> Self {}
}
```

## `Path::mnt`

What the entry is mounted on.

```rust
impl Path {
    pub fn mnt(&self) -> &Arc<VfsMount> {}
}
```

## `VfsMount`

A Filesystem as the runtime walks it, knowing nothing of where a guest finds it.

```rust
pub struct VfsMount {}
```

## `VfsMount::new`

A Filesystem mounted, allowing only reads where asked.

```rust
impl VfsMount {
    pub fn new(sb: Arc<SuperBlock>, read_only: bool) -> Arc<Self> {}
}
```

## `VfsMount::sb`

The Filesystem instance mounted.

```rust
impl VfsMount {
    pub fn sb(&self) -> &Arc<SuperBlock> {}
}
```

## `VfsMount::is_readonly`

Whether a change beneath it is refused.

```rust
impl VfsMount {
    pub fn is_readonly(&self) -> bool {}
}
```

## `Cred`

Who a call is made for, one per Invocation, holding its registered security modules.

```rust
pub struct Cred {}
```

## `Cred::new`

One with these security modules registered.

```rust
impl Cred {
    pub fn new(security: Vec<Arc<dyn SecurityOperations>>) -> Arc<Self> {}
}
```

## `prepare_creds`

The Cred an Invocation over these Mounts runs with: kobako's own rules, then each
Mount's Policy.

```rust
pub fn prepare_creds<'a>(mounts: impl IntoIterator<Item = &'a Mount>) -> Arc<Cred> {}
```

## `SecurityOperations`

What a security module answers. A hook a module lacks allows the call, and a
module can only refuse.

```rust
pub trait SecurityOperations: Send + Sync {}
```

## `SecurityOperations::inode_permission`

Whether a path beneath a mount may be reached with an access.

```rust
pub trait SecurityOperations {
    fn inode_permission(&self, _mnt: &Arc<VfsMount>, _path: &dyn Fn() -> String, _access: Access) -> Result<(), Error>;
}
```

## `SecurityOperations::file_open`

Whether a file of a kind may be opened.

```rust
pub trait SecurityOperations {
    fn file_open(&self, _kind: FileType) -> Result<(), FilesystemError>;
}
```

## `SecurityOperations::inode_readlink`

Whether where a link points may be read.

```rust
pub trait SecurityOperations {
    fn inode_readlink(&self) -> Result<(), FilesystemError>;
}
```

## `SecurityOperations::inode_follow_link`

Whether a walk beneath a mount may follow a link.

```rust
pub trait SecurityOperations {
    fn inode_follow_link(&self, _mnt: &Arc<VfsMount>) -> Result<(), FilesystemError>;
}
```

## `SecurityOperations::path_rename`

Whether an entry beneath a mount may move.

```rust
pub trait SecurityOperations {
    fn path_rename(&self, _mnt: &Arc<VfsMount>, _is_dir: bool) -> Result<(), FilesystemError>;
}
```

## `SecurityOperations::path_walk`

Whether a call may start from a Path.

```rust
pub trait SecurityOperations {
    fn path_walk(&self, _path: &Path) -> Result<(), Error>;
}
```

## `Path::metadata`

What the directory reports of itself.

```rust
impl Path {
    pub fn metadata(&self) -> Result<Metadata, Error> {}
}
```

## `File`

One open file or directory, as Linux's `struct file`. It holds where it was
opened, what answers for it, and what each descriptor of it shares.

```rust
pub struct File {}
```

## `File::root`

The root of a mounted Filesystem opened as a directory for a Cred.

```rust
impl File {
    pub fn root(mnt: &Arc<VfsMount>, cred: Arc<Cred>) -> Result<Self, FilesystemError> {}
}
```

## `File::path`

Where it was opened. A call relative to it starts here.

```rust
impl File {
    pub fn path(&self) -> &Path {}
}
```

## `File::path_init`

Where a walk relative to it starts, refusing anything but a directory.

```rust
impl File {
    pub fn path_init(&self) -> Result<&Path, FilesystemError> {}
}
```

## `File::is_directory`

Whether it is a directory.

```rust
impl File {
    pub fn is_directory(&self) -> bool {}
}
```

## `File::readable`

Whether it was opened to read.

```rust
impl File {
    pub fn readable(&self) -> bool {}
}
```

## `File::writable`

Whether it was opened to write.

```rust
impl File {
    pub fn writable(&self) -> bool {}
}
```

## `File::append`

Whether each write lands at the end.

```rust
impl File {
    pub fn append(&self) -> bool {}
}
```

## `File::nonblock`

Whether it asked not to wait.

```rust
impl File {
    pub fn nonblock(&self) -> bool {}
}
```

## `File::sync`

Whether each write is made durable before it answers.

```rust
impl File {
    pub fn sync(&self) -> bool {}
}
```

## `File::setfl`

Change the flags an open file may change, as `fcntl` takes only `SETFL_MASK`.

```rust
impl File {
    pub fn setfl(&mut self, append: bool, nonblock: bool) {}
}
```

## `Whence`

Where a seek counts from, as Linux's `SEEK_*`.

```rust
pub enum Whence {
    Set,
    Cur,
    End,
}
```

## `File::llseek`

Move its position and answer where it stands, as `vfs_llseek` does.

```rust
impl File {
    pub fn llseek(&mut self, offset: i64, whence: Whence) -> Result<u64, Error> {}
}
```

## `File::fadvise`

Take a hint about a range, as `generic_fadvise` refuses a pipe and a negative
length.

```rust
impl File {
    pub fn fadvise(&self, _offset: u64, len: u64) -> Result<(), FilesystemError> {}
}
```

## `File::fallocate`

Reserve a range, as `vfs_fallocate` refuses before it asks the file.

```rust
impl File {
    pub fn fallocate(&mut self, offset: u64, len: u64) -> Result<(), FilesystemError> {}
}
```

## `PollMask`

What a file would find ready, as Linux's `__poll_t`.

```rust
pub struct PollMask(u8);
```

## `PollMask::contains`

Whether this holds all of another.

```rust
impl PollMask {
    pub fn contains(self, mask: Self) -> bool {}
}
```

## `File::poll`

What a call would find ready, as `vfs_poll` asks the file.

```rust
impl File {
    pub fn poll(&self) -> PollMask {}
}
```

## `File::may_read`

Whether a read may reach it, as `vfs_read` reads `f_mode` before the buffer.

```rust
impl File {
    pub fn may_read(&self) -> Result<(), FilesystemError> {}
}
```

## `File::may_write`

Whether a write may reach it, as `vfs_write` reads `f_mode` before the buffer.

```rust
impl File {
    pub fn may_write(&self) -> Result<(), FilesystemError> {}
}
```

## `File::may_pread`

Whether a read from an offset may reach it, as `pread` refuses first.

```rust
impl File {
    pub fn may_pread(&self, pos: u64) -> Result<(), FilesystemError> {}
}
```

## `File::may_pwrite`

Whether a write at an offset may reach it, as `pwrite` refuses first.

```rust
impl File {
    pub fn may_pwrite(&self, pos: u64) -> Result<(), FilesystemError> {}
}
```

## `File::read`

Read from its position, moving it past what was read.

```rust
impl File {
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FilesystemError> {}
}
```

## `File::pread`

Read from an offset, leaving its position.

```rust
impl File {
    pub fn pread(&mut self, buf: &mut [u8], pos: u64) -> Result<usize, FilesystemError> {}
}
```

## `File::write`

Write at its position, moving it past what was written. Opened to append, it
lands at the end.

```rust
impl File {
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, FilesystemError> {}
}
```

## `File::pwrite`

Write at an offset, leaving its position. Opened to append, it lands at the end.

```rust
impl File {
    pub fn pwrite(&mut self, buf: &[u8], pos: u64) -> Result<usize, FilesystemError> {}
}
```

## `File::fsync`

Make what was written durable.

```rust
impl File {
    pub fn fsync(&mut self) -> Result<(), FilesystemError> {}
}
```

## `File::is_stream`

Whether it reads and writes what comes next, with no position, as a pipe's ends
do.

```rust
impl File {
    pub fn is_stream(&self) -> bool {}
}
```

## `File::getattr`

What it reports of itself.

```rust
impl File {
    pub fn getattr(&self) -> Result<Metadata, Error> {}
}
```

## `File::truncate`

Truncate or extend it through what it was opened on. Anything but a file open to
write refuses as invalid input.

```rust
impl File {
    pub fn truncate(&mut self, len: u64) -> Result<(), FilesystemError> {}
}
```

## `DirEntry::with_identity`

This entry naming a device and an inode number, as a stat of it reports them.

```rust
impl DirEntry {
    pub fn with_identity(self, dev: u64, ino: u64) -> Self {}
}
```

## `Profile::guest_ino`

The inode number a guest on this posture sees for an identity. A hermetic guest
sees none. Any other sees a hash under a key the process draws once.

```rust
impl Profile {
    pub fn guest_ino(self, identity: Option<Identity>) -> u64 {}
}
```

## `DirEmit`

One entry a listing hands its actor, as `dir_emit` does.

```rust
pub struct DirEmit {}
```

## `DirContext`

A listing under way, as `struct dir_context`. It holds where the listing stands
and who takes each entry.

```rust
pub struct DirContext {}
```

## `DirContext::new`

A listing from a position, each entry going to an actor.

```rust
impl DirContext {
    pub fn new(pos: u64, actor: &'a mut dyn FnMut(&DirEmit<'_>) -> bool) -> Self {}
}
```

## `File::iterate_dir`

Hand a listing the entries of the directory from its position, `.` and `..`
first, until its actor stops. A removed directory answers not found.

```rust
impl File {
    pub fn iterate_dir(&mut self, ctx: &mut DirContext<'_>) -> Result<(), Error> {}
}
```

## `FMode`

What an open file was opened for and what it can do.

```rust
pub struct FMode(u8);
```

## `FMode::contains`

Whether this holds all of another.

```rust
impl FMode {
    pub fn contains(self, mode: Self) -> bool {}
}
```

## `File::close`

Close it once the last descriptor goes, answering what its flush answers.

```rust
impl File {
    pub fn close(self) -> Result<(), FilesystemError> {}
}
```

## `PipeInodeInfo`

What a pipe holds: what was written, up to a capacity. An invocation's standard
input reads the Preamble from one, and each Channel's capture is one.

```rust
pub struct PipeInodeInfo {}
```

## `PipeInodeInfo::new`

An empty pipe holding at most this capacity's bytes.

```rust
impl PipeInodeInfo {
    pub fn new(capacity: usize) -> Arc<Self> {}
}
```

## `PipeInodeInfo::filled`

A pipe already holding these bytes, with no room for more.

```rust
impl PipeInodeInfo {
    pub fn filled(bytes: Vec<u8>) -> Arc<Self> {}
}
```

## `PipeInodeInfo::contents`

What was written so far.

```rust
impl PipeInodeInfo {
    pub fn contents(&self) -> Vec<u8> {}
}
```

## `create_pipe_files`

The read and the write end of a pipe, each an open file with no position.

```rust
pub fn create_pipe_files(pipe: &Arc<PipeInodeInfo>) -> (File, File) {}
```

## `OpenHow`

How to open a path, as `openat2` takes its flags.

```rust
pub struct OpenHow {}
```

## `do_file_open`

Open a path beneath a directory, as Linux's `do_file_open` serves `openat`.
It hands no descriptor out, so a caller holding a table reserves one first.

```rust
pub fn do_file_open(cred: &Arc<Cred>, start: &Path, path: &str, how: &OpenHow) -> Result<File, Error> {}
```

## `filename_linkat`

Give what a path names another name, as Linux's `filename_linkat` serves
`linkat`.

```rust
pub fn filename_linkat(cred: &Arc<Cred>, old_start: &Path, old_path: &str, new_start: &Path, new_path: &str, follow: bool) -> Result<(), Error> {}
```

## `filename_symlinkat`

Make a link at a path, as Linux's `filename_symlinkat` serves `symlinkat`.

```rust
pub fn filename_symlinkat(cred: &Arc<Cred>, target: &str, start: &Path, path: &str) -> Result<(), Error> {}
```

## `InodeOperations::link`

Give an entry another name in this directory. Without it the runtime answers
`NotPermitted`.

```rust
pub trait InodeOperations {
    fn link(&self, _old: &Dentry, _dentry: &Dentry) -> Option<Result<(), FilesystemError>>;
}
```

## `InodeOperations::symlink`

Make a link in this directory. Without it the runtime answers `NotPermitted`.

```rust
pub trait InodeOperations {
    fn symlink(&self, _dentry: &Dentry, _target: &str) -> Option<Result<(), FilesystemError>>;
}
```

## `filename_mkdirat`

Make a directory at a path beneath a directory, as Linux's `filename_mkdirat`
serves `mkdirat`.

```rust
pub fn filename_mkdirat(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<(), Error> {}
```

## `filename_rmdir`

Remove the empty directory at a path beneath a directory.

```rust
pub fn filename_rmdir(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<(), Error> {}
```

## `filename_unlinkat`

Remove the file at a path beneath a directory.

```rust
pub fn filename_unlinkat(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<(), Error> {}
```

## `filename_renameat2`

Move an entry within one Mount.

```rust
pub fn filename_renameat2(cred: &Arc<Cred>, from: &Path, from_path: &str, to: &Path, to_path: &str) -> Result<(), Error> {}
```

## `vfs_fstatat`

What a path names, a link at its end followed where asked and reported as
itself otherwise.

```rust
pub fn vfs_fstatat(cred: &Arc<Cred>, start: &Path, path: &str, follow: bool) -> Result<Metadata, Error> {}
```

## `do_readlinkat`

Where a link points, asked of the security hooks first. kobako's own rules refuse
it as unsupported.

```rust
pub fn do_readlinkat(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<String, Error> {}
```

## `do_utimes`

Refuse setting timestamps as unsupported once the path is known to stand.

```rust
pub fn do_utimes(cred: &Arc<Cred>, start: &Path, path: &str, follow: bool, times: &Iattr) -> Result<(), Error> {}
```

## `vfs_utimes`

Set times on what a Path names, as Linux's `vfs_utimes` does for a path and a
descriptor alike. An entry without `setattr` answers unsupported.

```rust
pub fn vfs_utimes(cred: &Arc<Cred>, path: &Path, times: &Iattr) -> Result<(), Error> {}
```

## `Iattr::times`

A change setting the times named, leaving any other as it stands.

```rust
impl Iattr {
    pub fn times(atime: Option<SetTime>, mtime: Option<SetTime>) -> Self {}
}
```

## `SetTime`

A time a change sets, either the moment it lands or the time it carries.

```rust
pub enum SetTime {
    Now,
    At(SystemTime),
}
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

The Host App's decision for each path the Guest opens or changes, made after a
Mount's access allows it.

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

## `InodeOperations::dentry_ops`

What the runtime asks of a Filesystem's root once, to give every Dentry of it.
A Filesystem without one keeps every name valid.

```rust
pub trait InodeOperations {
    fn dentry_ops(&self) -> Option<Arc<dyn DentryOperations>>;
}
```

## `DentryOperations`

What a Filesystem answers about a name rather than an entry. A name may stand
for nothing, so no entry can answer for it.

```rust
pub trait DentryOperations: Send + Sync {}
```

## `DentryOperations::revalidate`

Whether a Dentry the runtime still holds in a directory names what it recorded.
A false answer has the runtime drop the Dentry and take the name afresh.

```rust
pub trait DentryOperations {
    fn revalidate(
        &self,
        dir: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Result<bool, FilesystemError>;
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

Open this entry, truncating a file where the options say to. A directory opens
to be listed. The runtime refuses a link before it asks.

```rust
pub trait InodeOperations {
    fn open(&self, _dentry: &Dentry, _options: &OpenOptions) -> Option<Result<Box<dyn FileOperations>, FilesystemError>>;
}
```

## `InodeOperations::create`

Make the file a Dentry names in this directory and open it. A directory without
it answers permission denied.

```rust
pub trait InodeOperations {
    fn create(&self, _dentry: &Dentry, _options: &OpenOptions) -> Option<Result<Box<dyn FileOperations>, FilesystemError>>;
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

## `Iattr`

What a change sets of an entry.

```rust
pub struct Iattr {}
```

## `Iattr::size`

A change truncating or extending a file, extending with zeros.

```rust
impl Iattr {
    pub fn size(len: u64) -> Self {}
}
```

## `InodeOperations::setattr`

Apply a change to this entry, as it stands even once its name is gone. An entry
without it refuses as invalid input.

```rust
pub trait InodeOperations {
    fn setattr(&self, _dentry: &Dentry, _attr: &Iattr) -> Option<Result<(), FilesystemError>>;
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

## `FileOperations`

What one open file answers, as Linux's `file_operations`. The runtime keeps its
position, so each read and write names its offset. An operation a file lacks
answers as Linux's VFS answers for a NULL in the table.

```rust
pub trait FileOperations {}
```

## `Kiocb`

Where one read or write lands, and where the file left off once it did.

```rust
pub struct Kiocb {}
```

## `Kiocb::at`

A call landing at an offset.

```rust
impl Kiocb {
    pub fn at(pos: u64) -> Self {}
}
```

## `Kiocb::with_append`

The same call, landing at the file's end as it stands when the write lands.

```rust
impl Kiocb {
    pub fn with_append(self) -> Self {}
}
```

## `FileOperations::read_iter`

Read into the buffer from where the call lands, answering how much was read. It
moves the position past what it read, unless the file has no positions.

```rust
pub trait FileOperations {
    fn read_iter(&mut self, _iocb: &mut Kiocb, _buf: &mut [u8]) -> Result<usize, FilesystemError>;
}
```

## `FileOperations::write_iter`

Write the buffer where the call lands, answering how much was written. It moves
the position past what it wrote, unless the file has no positions. A call landing
at the end that the file cannot take as one step answers `Unsupported`.

```rust
pub trait FileOperations {
    fn write_iter(&mut self, _iocb: &mut Kiocb, _buf: &[u8]) -> Result<usize, FilesystemError>;
}
```

## `FileOperations::iterate_shared`

The entries of this directory, without `.` or `..`. A file without it answers
not a directory.

```rust
pub trait FileOperations {
    fn iterate_shared(&mut self, _dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError>;
}
```

## `FileOperations::fallocate`

Reserve a range. A file without it answers `Unsupported`, as `vfs_fallocate` does.

```rust
pub trait FileOperations {
    fn fallocate(&mut self, _offset: u64, _len: u64) -> Result<(), FilesystemError>;
}
```

## `FileOperations::poll`

What a call would find ready on this end. A file without it is always ready, as
`vfs_poll` answers.

```rust
pub trait FileOperations {
    fn poll(&self, _mode: FMode) -> PollMask;
}
```

## `FileOperations::fsync`

Make what was written durable. A file with nothing to flush answers success
itself; one without it refuses as invalid input.

```rust
pub trait FileOperations {
    fn fsync(&mut self) -> Result<(), FilesystemError>;
}
```

## `FileOperations::flush`

Answer, as the guest closes the file, whether what it held was kept. The default
has nothing to flush. Whatever else the file holds goes when it is dropped.

```rust
pub trait FileOperations {
    fn flush(&mut self) -> Result<(), FilesystemError>;
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

## `register_blocking`

Have every thread of the process wait through the given Blocking. A second
registration is handed back.

```rust
pub fn register_blocking(blocking: Box<dyn Blocking>) -> Result<(), Box<dyn Blocking>> {}
```

## `Blocking`

How a thread waits for a lock another thread holds. A frontend whose threads
share a lock of their own gives it up for the wait.

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
