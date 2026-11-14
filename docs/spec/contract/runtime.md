# Runtime seams

What a Rust SDK user implements to give the Guest a Filesystem or a security
module. It also covers the values a run reports back. A Wasm engine of one's own is
experimental, so the runtime promises it nothing here.

## Includes

- `crates/kobako-runtime/src/errno.rs`
- `crates/kobako-runtime/src/error.rs`
- `crates/kobako-runtime/src/filesystem/bind.rs`
- `crates/kobako-runtime/src/filesystem/overlay.rs`
- `crates/kobako-runtime/src/filesystem/rdir.rs`
- `crates/kobako-runtime/src/filesystem/tmpfs.rs`
- `crates/kobako-runtime/src/limits.rs`
- `crates/kobako-runtime/src/policy.rs`
- `crates/kobako-runtime/src/mount.rs`
- `crates/kobako-runtime/src/net/backends.rs`
- `crates/kobako-runtime/src/net/family.rs`
- `crates/kobako-runtime/src/net/lookup.rs`
- `crates/kobako-runtime/src/profile.rs`
- `crates/kobako-runtime/src/cred.rs`
- `crates/kobako-runtime/src/security/hooks.rs`
- `crates/kobako-runtime/src/snapshot.rs`
- `crates/kobako-runtime/src/vfs.rs`
- `crates/kobako-runtime/src/vfs/attr.rs`
- `crates/kobako-runtime/src/vfs/dentry.rs`
- `crates/kobako-runtime/src/vfs/mount.rs`
- `crates/kobako-runtime/src/vfs/file.rs`
- `crates/kobako-runtime/src/vfs/super_block.rs`
- `crates/kobako-runtime/src/vfs/inode.rs`
- `crates/kobako-runtime/src/vfs/openat2.rs`
- `crates/kobako-runtime/src/vfs/readdir.rs`
- `crates/kobako-runtime/src/vfs/stat.rs`
- `crates/kobako-runtime/src/wait.rs`

## `Usage`

What one invocation consumed of its time and memory.

```rust
pub struct Usage {}
```

## `Profile`

The isolation profile an engine declares, ordered so a floor can be compared
against it.

```rust
pub enum Profile {}
```

## `Backends`

What every network entry of an Invocation dials and looks names up through,
beneath the rules its entries make.

```rust
pub struct Backends {}
```

## `Backends::new`

Dial through one Protocol Family and look names up through one Resolver.

```rust
impl Backends {
    pub fn new(family: Arc<dyn ProtocolFamily>, resolver: Arc<dyn Resolver>) -> Self {}
}
```

## `ProtocolFamily`

What dials the host network, handed only an address the rules admitted.

```rust
pub trait ProtocolFamily: Send + Sync {}
```

## `ProtocolFamily::connect`

A non-blocking stream to an address, whose connect may still be under way.

```rust
pub trait ProtocolFamily {
    fn connect(&self, addr: SocketAddr) -> io::Result<TcpStream>;
}
```

## `ProtocolFamily::local_addresses`

The host's own addresses, read at each connect; a family that cannot say has
every connect refused.

```rust
pub trait ProtocolFamily {
    fn local_addresses(&self) -> io::Result<Vec<IpAddr>>;
}
```

## `ProtocolFamily::connect_datagram`

A non-blocking datagram socket connected to one peer; a family with no
datagrams answers `Unsupported`.

```rust
pub trait ProtocolFamily {
    fn connect_datagram(&self, _addr: SocketAddr) -> io::Result<UdpSocket>;
}
```

## `Resolver`

What answers a name, each address it answers judged again at connect.

```rust
pub trait Resolver: Send + Sync {}
```

## `Resolver::resolve`

The addresses a fully qualified name resolves to.

```rust
pub trait Resolver {
    fn resolve(&self, name: &str) -> std::io::Result<Vec<IpAddr>>;
}
```

## `Limits`

The caps a Sandbox runs under, each one off when it is none.

```rust
pub struct Limits {}
```

## `SetupError`

Why an invocation could not start before the engine ran.

```rust
pub enum SetupError {}
```

## `SuperBlock`

One Filesystem instance, as Linux's `super_block`. A Host App names it
`filesystem::Filesystem`. Its Mounts share the directories a Guest reaches
beneath it, as bind mounts share a super block.

```rust
pub struct SuperBlock {}
```

## `SuperBlock::new`

The instance a Filesystem makes, set up as its `fill_super` asks, or why it cannot stand.

```rust
impl SuperBlock {
    pub fn new(fs: Arc<dyn FileSystemType>) -> Result<Arc<Self>, Errno> {}
}
```

## `SuperBlock::d_make_root`

Make a root Dentry for an Inode and set it as the instance's root, from
`FileSystemType::fill_super`.

```rust
impl SuperBlock {
    pub fn d_make_root(&mut self, root: Arc<Inode>) {}
}
```

## `SuperBlock::set_default_d_op`

What every Dentry of the instance made after it answers through, set from
`FileSystemType::fill_super` before `SuperBlock::d_make_root`.

```rust
impl SuperBlock {
    pub fn set_default_d_op(&mut self, ops: Arc<dyn DentryOperations>) {}
}
```

## `SuperBlock::set_rdonly`

Refuses every change beneath the instance, set from `FileSystemType::fill_super`.

```rust
impl SuperBlock {
    pub fn set_rdonly(&mut self) {}
}
```

## `SuperBlock::set_dontcache`

Keeps no Dentry of the instance once nothing holds it, set from
`FileSystemType::fill_super`.

```rust
impl SuperBlock {
    pub fn set_dontcache(&mut self) {}
}
```

## `SuperBlock::rdonly`

Whether nothing beneath the instance changes, however it is mounted.

```rust
impl SuperBlock {
    pub fn rdonly(&self) -> bool {}
}
```

## `SuperBlock::root`

The root Dentry of the instance, once `FileSystemType::fill_super` made it.

```rust
impl SuperBlock {
    pub fn root(&self) -> Option<&Arc<Dentry>> {}
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

Where the name stands now beneath the Filesystem's root, from a leading `/`,
which alone names the root.

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
    pub fn add(&self, inode: Option<Arc<Inode>>) {}
}
```

## `Dentry::inode`

What the name stands for, none while nothing was recorded.

```rust
impl Dentry {
    pub fn inode(&self) -> Option<Arc<Inode>> {}
}
```

## `Dentry::in_lookup`

Whether no lookup has answered for the name yet.

```rust
impl Dentry {
    pub fn in_lookup(&self) -> bool {}
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

The identity a `Kstat` names, none where its inode number of zero reports none.

```rust
impl Identity {
    pub fn of(stat: &Kstat) -> Option<Self> {}
}
```

## `Identity::onto`

The same `Kstat` naming this identity, for an open file a Filesystem answers
for apart from its path.

```rust
impl Identity {
    pub fn onto(self, stat: Kstat) -> Kstat {}
}
```

## `Inode`

One entry of a Filesystem, as a lookup records it on a Dentry.

```rust
pub struct Inode {}
```

## `Inode::new`

The entry a `Kstat` reports, answered for by its operations.

```rust
impl Inode {
    pub fn new(stat: &Kstat, ops: Arc<dyn InodeOperations>) -> Arc<Self> {}
}
```

## `Inode::with_ops`

This entry answered for by other operations, as a wrapper wraps what it finds.

```rust
impl Inode {
    pub fn with_ops(&self, ops: Arc<dyn InodeOperations>) -> Arc<Self> {}
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

## `VfsMount`

A Filesystem as the runtime walks it, knowing nothing of where a guest finds it.

```rust
pub struct VfsMount {}
```

## `Cred`

Who a call is made for, one per Invocation, holding its registered security modules.
A Wasm Driver gets one only from `prepare_creds_within`, so kobako's own rules always come first.

```rust
pub struct Cred {}
```

## `SecurityOperations`

What a security module answers. A hook a module lacks allows the call, and a
module can only refuse.

```rust
pub trait SecurityOperations: Send + Sync {}
```

## `SecurityPath`

A path a hook is asked about: its mount, and its name built only for a module
reading it. The name is what `Dentry::path_raw` answers for the entry when the
module asks.

```rust
pub struct SecurityPath {}
```

## `SecurityOperations::inode_permission`

Whether a call may do what a `Mask` asks of an entry, once the entry allowed it.

```rust
pub trait SecurityOperations {
    fn inode_permission(&self, _dentry: &Dentry, _mask: Mask) -> Result<(), Errno>;
}
```

## `SecurityOperations::path_mknod`

Whether a file may be made at a path.

```rust
pub trait SecurityOperations {
    fn path_mknod(&self, _path: SecurityPath<'_>) -> Result<(), Errno>;
}
```

## `SecurityOperations::path_mkdir`

Whether a directory may be made at a path.

```rust
pub trait SecurityOperations {
    fn path_mkdir(&self, _path: SecurityPath<'_>) -> Result<(), Errno>;
}
```

## `SecurityOperations::path_rmdir`

Whether the directory at a path may go.

```rust
pub trait SecurityOperations {
    fn path_rmdir(&self, _path: SecurityPath<'_>) -> Result<(), Errno>;
}
```

## `SecurityOperations::path_unlink`

Whether the file at a path may go.

```rust
pub trait SecurityOperations {
    fn path_unlink(&self, _path: SecurityPath<'_>) -> Result<(), Errno>;
}
```

## `SecurityOperations::path_symlink`

Whether a link may be made at a path.

```rust
pub trait SecurityOperations {
    fn path_symlink(&self, _path: SecurityPath<'_>) -> Result<(), Errno>;
}
```

## `SecurityOperations::path_link`

Whether what one path names may also go by another.

```rust
pub trait SecurityOperations {
    fn path_link(&self, _old: SecurityPath<'_>, _new: SecurityPath<'_>) -> Result<(), Errno>;
}
```

## `SecurityOperations::file_open`

Whether a file may be opened for a mode as the open asks, a file the open made
included. What the module answers it keeps on the open file.

```rust
pub trait SecurityOperations {
    fn file_open(&self, _path: SecurityPath<'_>, _kind: FileType, _mode: FMode, _how: &OpenHow) -> Result<FileSecurity, Errno>;
}
```

## `SecurityOperations::file_truncate`

Whether an open file may be truncated, judged from what the module kept on it
as it opened.

```rust
pub trait SecurityOperations {
    fn file_truncate(&self, _kept: &FileSecurity) -> Result<(), Errno>;
}
```

## `SecurityOperations::path_utimes`

Whether times may be set on the entry at a path.

```rust
pub trait SecurityOperations {
    fn path_utimes(&self, _path: SecurityPath<'_>) -> Result<(), Errno>;
}
```

## `SecurityOperations::inode_readlink`

Whether where the link a Dentry names points may be read.

```rust
pub trait SecurityOperations {
    fn inode_readlink(&self, _dentry: &Dentry) -> Result<(), Errno>;
}
```

## `SecurityOperations::socket_connect`

Whether a socket may connect where a Connection names, judged on what the Guest
named.

```rust
pub trait SecurityOperations {
    fn socket_connect(&self, _connection: &Connection<'_>) -> Result<(), Errno>;
}
```

## `SecurityOperations::socket_bind`

Whether a socket may bind an address.

```rust
pub trait SecurityOperations {
    fn socket_bind(&self, _address: SocketAddr) -> Result<(), Errno>;
}
```

## `SecurityOperations::path_rename`

Whether what one path names may move to another, the dentries of each given.

```rust
pub trait SecurityOperations {
    fn path_rename(&self, _old: SecurityPath<'_>, _new: SecurityPath<'_>, _dentries: (&Arc<Dentry>, &Arc<Dentry>)) -> Result<(), Errno>;
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

## `FileOperations::llseek`

Where a seek leaves this file. A file without it takes no seek; one that seeks
as any file does answers through `generic_file_llseek`.

```rust
pub trait FileOperations {
    fn llseek(&mut self, _dentry: &Dentry, _pos: u64, _offset: i64, _whence: Whence) -> Result<u64, Errno>;
}
```

## `generic_file_llseek`

Where a seek leaves a file. It counts from the start, the position, or the reported
end, and refuses a place past the largest offset.

```rust
pub fn generic_file_llseek(dentry: &Dentry, pos: u64, offset: i64, whence: Whence) -> Result<u64, Errno> {}
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

## `PollMask::IN`

A read does not wait.

```rust
impl PollMask {
    pub const IN: Self;
}
```

## `PollMask::OUT`

A write does not wait.

```rust
impl PollMask {
    pub const OUT: Self;
}
```

## `PollMask::ERR`

The file failed, and a call answers its error.

```rust
impl PollMask {
    pub const ERR: Self;
}
```

## `PollMask::HUP`

The peer hung up, and a read answers the end.

```rust
impl PollMask {
    pub const HUP: Self;
}
```

## `DirEntry::with_identity`

This entry naming a device and an inode number, as a stat of it reports them.

```rust
impl DirEntry {
    pub fn with_identity(self, dev: u64, ino: u64) -> Self {}
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

## `FMode::READ`

The file was opened to read.

```rust
impl FMode {
    pub const READ: Self;
}
```

## `FMode::WRITE`

The file was opened to write.

```rust
impl FMode {
    pub const WRITE: Self;
}
```

## `FMode::LSEEK`

The file has a position that a seek can move.

```rust
impl FMode {
    pub const LSEEK: Self;
}
```

## `FMode::PREAD`

The file reads from an offset that the call names.

```rust
impl FMode {
    pub const PREAD: Self;
}
```

## `FMode::PWRITE`

The file writes at an offset that the call names.

```rust
impl FMode {
    pub const PWRITE: Self;
}
```

## `FMode::STREAM`

The file reads and writes the next bytes, and has no position.

```rust
impl FMode {
    pub const STREAM: Self;
}
```

## `FMode::CREATED`

The open that opened the file also made it.

```rust
impl FMode {
    pub const CREATED: Self;
}
```

## `FMode::PATH`

The file stands only for its path, and does not read or write.

```rust
impl FMode {
    pub const PATH: Self;
}
```

## `OpenHow`

How to open a path, as `openat2` takes its flags.

```rust
pub struct OpenHow {}
```

## `InodeOperations::link`

Give an entry another name in this directory. Without it the runtime answers
`NotPermitted`.

```rust
pub trait InodeOperations {
    fn link(&self, _old: &Dentry, _dentry: &Dentry) -> Option<Result<(), Errno>>;
}
```

## `InodeOperations::symlink`

Make a link in this directory. Without it the runtime answers `NotPermitted`.

```rust
pub trait InodeOperations {
    fn symlink(&self, _dentry: &Dentry, _target: &str) -> Option<Result<(), Errno>>;
}
```

## `Iattr::times`

A change setting the times named, leaving any other as it stands.

```rust
impl Iattr {
    pub fn times(atime: Option<SetTime>, mtime: Option<SetTime>) -> Self {}
}
```

## `Iattr::time_of`

The time a `SetTime` names, now read as the moment the change lands.

```rust
impl Iattr {
    pub fn time_of(&self, set: SetTime) -> SystemTime {}
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

The Host App's decision for one request a Grant's access already allows, so it
only refuses more. Each kind of Grant brings its own request, and a closure
over that request is a Policy.

```rust
pub trait Policy<R: ?Sized> {}
```

## `Policy::allows`

Whether the Guest may go on with the request.

```rust
pub trait Policy<R: ?Sized> {
    fn allows(&self, request: &R) -> bool;
}
```

## `MountRequest`

One path the Guest opens or changes beneath a Mount. It holds the absolute
guest path and the access asked for there.

```rust
pub struct MountRequest<'a> {
    pub path: &'a str,
    pub access: Access,
}
```

## `InodeOperations`

What one entry of a Filesystem answers, as Linux's `inode_operations`. Only
`getattr` is required. An operation an entry lacks answers by default as Linux's
VFS answers for it. A wrapper wraps each entry its `lookup` finds.

```rust
pub trait InodeOperations {}
```

## `InodeOperations::getattr`

What this entry reports of itself. A link reports itself.

```rust
pub trait InodeOperations {
    fn getattr(&self, dentry: &Dentry) -> Result<Kstat, Errno>;
}
```

## `InodeOperations::lookup`

What stands at the name a Dentry carries in this directory. An entry without
it is no directory to walk through.

```rust
pub trait InodeOperations {
    fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, Errno>;
}
```

## `InodeOperations::permission`

Whether a call may do what a `Mask` asks of this entry. An entry without it allows
the call, and its backend answers once the call is made.

```rust
pub trait InodeOperations {
    fn permission(&self, _dentry: &Dentry, _mask: Mask) -> Result<(), Errno>;
}
```

## `Mask`

What a call asks of an entry: to pass through, change, append to, read, or open
it.

```rust
pub struct Mask {}
```

## `Mask::contains`

Whether this holds all of another mask.

```rust
impl Mask {
    pub fn contains(self, mask: Self) -> bool {}
}
```

## `Mask::EXEC`

The call passes through a directory.

```rust
impl Mask {
    pub const EXEC: Self;
}
```

## `Mask::WRITE`

The call changes the entry.

```rust
impl Mask {
    pub const WRITE: Self;
}
```

## `Mask::READ`

The call reads the entry.

```rust
impl Mask {
    pub const READ: Self;
}
```

## `Mask::OPEN`

The call opens the entry.

```rust
impl Mask {
    pub const OPEN: Self;
}
```

## `Mask::APPEND`

The call changes the entry only at its end, and always comes with
`Mask::WRITE`.

```rust
impl Mask {
    pub const APPEND: Self;
}
```

## `FileSystemType`

What makes an instance of a Filesystem.

```rust
pub trait FileSystemType: Send + Sync + 'static {}
```

## `FileSystemType::fill_super`

What the runtime asks once to set up an instance and make its root, answering why it
cannot stand.

```rust
pub trait FileSystemType {
    fn fill_super(self: Arc<Self>, sb: &mut SuperBlock) -> Result<(), Errno>;
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
    ) -> Result<bool, Errno>;
}
```

## `DentryOperations::delete`

Whether a Dentry goes once nothing holds it, rather than stay for the next walk.
A name whose `revalidate` cannot tell a change must answer true.

```rust
pub trait DentryOperations {
    fn delete(&self, _dentry: &Dentry) -> bool;
}
```

## `InodeOperations::get_link`

Where this link points, for the runtime to follow. An entry without it answers
invalid input.

```rust
pub trait InodeOperations {
    fn get_link(&self, _dentry: &Dentry) -> Result<String, Errno>;
}
```

## `InodeOperations::open`

Open this entry. A directory opens to be listed. The runtime refuses a link
before it asks, and empties a file through `setattr` afterwards. An entry without
it opens with no operations.

```rust
pub trait InodeOperations {
    fn open(&self, _dentry: &Dentry, _how: &OpenHow) -> Result<Box<dyn FileOperations>, Errno>;
}
```

## `InodeOperations::create`

Make the file a Dentry names in this directory and record it on that Dentry. A
directory without it answers permission denied.

```rust
pub trait InodeOperations {
    fn create(&self, _dentry: &Dentry) -> Result<(), Errno>;
}
```

## `InodeOperations::atomic_open`

Look up, make, and open the last name of an open in one step. The runtime asks
it for a name still in lookup, and for a negative name an open may make. A
Dentry still in lookup is looked up first and may be handed back. Only an open
that may make a file makes one.

A directory without it looks up, then makes the file through `create`. The
runtime asks its security modules before it opens the file.

```rust
pub trait InodeOperations {
    fn atomic_open(&self, dentry: &Dentry, how: &OpenHow) -> Result<Finish, Errno>;
}
```

## `InodeOperations::mkdir`

Make the directory a Dentry names in this one. A directory without it answers
not permitted.

```rust
pub trait InodeOperations {
    fn mkdir(&self, _dentry: &Dentry) -> Result<(), Errno>;
}
```

## `InodeOperations::rmdir`

Remove the empty directory a Dentry names in this one. A directory without it
answers not permitted.

```rust
pub trait InodeOperations {
    fn rmdir(&self, _dentry: &Dentry) -> Result<(), Errno>;
}
```

## `InodeOperations::unlink`

Remove the file a Dentry names in this directory. A directory without it
answers not permitted.

```rust
pub trait InodeOperations {
    fn unlink(&self, _dentry: &Dentry) -> Result<(), Errno>;
}
```

## `Iattr`

What a change sets of an entry, and the open file a truncate is made through.

```rust
pub struct Iattr<'a> {}
```

## `Iattr::size`

A change truncating or extending a file, extending with zeros, which moves its
modification time to the moment it lands.

```rust
impl Iattr {
    pub fn size(len: u64) -> Self {}
}
```

## `InodeOperations::setattr`

Apply a change to this entry, as it stands even once its name is gone. An entry
without it refuses a size as invalid input and a time as unsupported.

```rust
pub trait InodeOperations {
    fn setattr(&self, _dentry: &Dentry, attr: &Iattr) -> Result<(), Errno>;
}
```

## `InodeOperations::rename`

Move what one Dentry names to another in a target directory, refusing what
rename(2) refuses at the target. The runtime has already checked the source
and never hands a move onto its own name. A target of another type crosses
devices, and a flag this directory does not take is invalid input. A directory
without it answers not permitted.

```rust
pub trait InodeOperations {
    fn rename(&self, _from: &Dentry, _to_dir: &dyn InodeOperations, _to: &Dentry, _flag: Option<RenameFlag>) -> Result<(), Errno>;
}
```

## `RenameFlag`

What a move asks of its target, as `renameat2`'s flags. It refuses a replacement,
swaps the two, or leaves a whiteout.

```rust
pub enum RenameFlag {}
```

## `Finish`

What `atomic_open` did with a name. It opened it, opened it under another
spelling, left the open to the runtime, or handed back its lookup. Opening the
name and leaving the open say whether it made the file.

```rust
pub enum Finish {}
```

## `Lookup`

What one name in a directory turned out to be: an entry, nothing, or an entry
stored under another spelling. The last carries both.

```rust
pub enum Lookup {}
```

## `FileOperations`

What one open file answers, as Linux's `file_operations`. The runtime keeps its
position, so each read and write names its offset. An operation a file lacks
answers as Linux's VFS answers for a NULL in the table. Any thread may hold it,
and a change made through it hands it to its Filesystem.

```rust
pub trait FileOperations: Any + Send + Sync {}
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

## `generic_write_sync`

Make what a write changed durable through the file where the call asks. A
Filesystem's write calls it once the write landed.

```rust
pub fn generic_write_sync<F: FileOperations + ?Sized>(file: &mut F, iocb: &Kiocb, written: usize) -> Result<usize, Errno> {}
```

## `FileOperations::read_iter`

Read into the buffer from where the call lands, answering how much was read. It
moves the position past what it read, unless the file has no positions.

```rust
pub trait FileOperations {
    fn read_iter(&mut self, _iocb: &mut Kiocb, _buf: &mut [u8]) -> Result<usize, Errno>;
}
```

## `FileOperations::write_iter`

Write the buffer where the call lands, answering how much was written. It moves
the position past what it wrote, unless the file has no positions. A call landing
at the end that the file cannot take as one step answers `Unsupported`.

```rust
pub trait FileOperations {
    fn write_iter(&mut self, _iocb: &mut Kiocb, _buf: &[u8]) -> Result<usize, Errno>;
}
```

## `FileOperations::iterate_shared`

Hand a listing the entries of this directory from its position, `.` and `..`
first, until its actor stops. A file without it answers not a directory.

```rust
pub trait FileOperations {
    fn iterate_shared(&mut self, _dentry: &Dentry, _ctx: &mut DirContext<'_>) -> Result<(), Errno>;
}
```

## `DirContext::pos`

Where the listing stands.

```rust
impl DirContext {
    pub fn pos(&self) -> u64 {}
}
```

## `DirContext::cred`

Who the listing is for.

```rust
impl DirContext {
    pub fn cred(&self) -> &Arc<Cred> {}
}
```


## `DirContext::set_pos`

Move the listing to where the next entry stands, or past the last.

```rust
impl DirContext {
    pub fn set_pos(&mut self, pos: u64) {}
}
```

## `DirContext::dir_emit`

Hand the actor an entry standing where the listing stands, answering whether to
go on. The Filesystem moves the listing past what the actor took.

```rust
impl DirContext {
    pub fn dir_emit(&mut self, entry: &DirEntry) -> bool {}
}
```

## `DirEmit::to_dir_entry`

This entry as a list keeps one.

```rust
impl DirEmit {
    pub fn to_dir_entry(&self) -> DirEntry {}
}
```

## `dir_emit_dots`

Hand a listing the `.` and `..` it has yet to reach, answering whether to go on.
Each names the inode its dentry holds.

```rust
pub fn dir_emit_dots(dentry: &Dentry, ctx: &mut DirContext<'_>) -> bool {}
```

## `generic_read_dir`

What a directory's read answers.

```rust
pub fn generic_read_dir() -> Result<usize, Errno> {}
```

## `Rdir`

What a directory read whole at once holds while a listing goes on.

```rust
pub struct Rdir {}
```

## `Rdir::iterate`

Hand a listing the directory from its position, counting what it holds until it
ends. A fresh listing reads it, then stats it and its parent for `.` and `..`.

Either failure answers before any entry. A listing going on reads what it began
with.

```rust
impl Rdir {
    pub fn iterate(&mut self, dentry: &Dentry, ctx: &mut DirContext<'_>, read: impl FnOnce() -> Result<Vec<DirEntry>, Errno>) -> Result<(), Errno> {}
}
```

## `FileOperations::fallocate`

Reserve a range. A file without it answers `Unsupported`, as `vfs_fallocate` does.

```rust
pub trait FileOperations {
    fn fallocate(&mut self, _offset: u64, _len: u64) -> Result<(), Errno>;
}
```

## `FileOperations::poll`

What a call would find ready on this end. A file that can become ready later
registers its wait queue on the table through `poll_wait`. A file without it
is always ready, as `vfs_poll` answers.

```rust
pub trait FileOperations {
    fn poll(&self, _mode: FMode, _table: Option<&mut PollTable>) -> PollMask;
}
```

## `FileOperations::fsync`

Make what was written durable. A file with nothing to flush answers success
itself; one without it refuses as invalid input.

```rust
pub trait FileOperations {
    fn fsync(&mut self, _datasync: bool) -> Result<(), Errno>;
}
```

## `FileOperations::flush`

Answer, as the guest closes the file, whether what it held was kept. The default
has nothing to flush. Whatever else the file holds goes when it is dropped.

```rust
pub trait FileOperations {
    fn flush(&mut self) -> Result<(), Errno>;
}
```

## `FileOperations::is_stream`

Whether the file reads and writes what comes next, with no position. The
default takes a position.

```rust
pub trait FileOperations {
    fn is_stream(&self) -> bool;
}
```

## `FileType`

What a path names: a file, a directory, a link, or another kind a host holds, such as a FIFO.

```rust
pub enum FileType {}
```

## `Kstat`

What a path names, its size, its identity, and its times.

```rust
pub struct Kstat {}
```

## `Kstat::new`

What a path names and its size, as a Filesystem reports it.

```rust
impl Kstat {
    pub fn new(file_type: FileType, size: u64) -> Self {}
}
```

## `Kstat::with_identity`

The same `Kstat` naming the device and inode numbers a Filesystem gives what
the path holds.

```rust
impl Kstat {
    pub fn with_identity(self, dev: u64, ino: u64) -> Self {}
}
```

## `Kstat::with_nlink`

The same `Kstat` counting the names a Filesystem gives the entry.

```rust
impl Kstat {
    pub fn with_nlink(self, nlink: u64) -> Self {}
}
```

## `Kstat::with_times`

The same `Kstat` recording when the entry was last read, written, and
changed.

```rust
impl Kstat {
    pub fn with_times(self, atime: Option<SystemTime>, mtime: Option<SystemTime>, ctime: Option<SystemTime>) -> Self {}
}
```

## `Kstat::unique_device`

A device number for a Filesystem with none of its own. No earlier call
returned it, and no host reports it.

```rust
impl Kstat {
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

## `Errno`

Why a call failed, under the POSIX name Linux answers it with. The engine
hands the Guest the matching error code.

```rust
pub enum Errno {}
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

## `Bind::with_exclusive`

This Bind trusting the names it found, when only the guest changes its
directory.

```rust
impl Bind {
    pub fn with_exclusive(self, exclusive: bool) -> Self {}
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
    ) -> Result<Self, Errno> {}
}
```

## `WaitQueueHead`

Where the waiters for one event stand, as Linux's `wait_queue_head_t`.

```rust
pub struct WaitQueueHead {}
```

## `WaitQueueHead::wake_up`

Wake every waiter standing on the queue once its event arrived.

```rust
impl WaitQueueHead {
    pub fn wake_up(&self) {}
}
```

## `PollTable`

The queues one poll registered on, as Linux's `poll_table`. Dropping it leaves
each queue.

```rust
pub struct PollTable {}
```

## `poll_wait`

Have a poll woken by the given queue, as a file's `->poll` calls `poll_wait`.
A poll that will not sleep passes no table.

```rust
pub fn poll_wait(queue: &Arc<WaitQueueHead>, table: Option<&mut PollTable>) {}
```

