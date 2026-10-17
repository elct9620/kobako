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

A host resource a Frontend hands the engine for one invocation.

```rust
pub enum Grant {}
```

## `Grant::kind`

Which kind of Grant this is, as `Runtime::supports` names it.

```rust
impl Grant {
    pub fn kind(&self) -> GrantKind {}
}
```

## `GrantKind`

The kinds of Grant an engine may support.

```rust
pub enum GrantKind {}
```

## `Access`

What the Guest may do beneath a filesystem Grant: read, or read and write.

```rust
pub enum Access {}
```

## `Policy`

The Host App's decision for each path the Guest opens or changes, made in a
filesystem Grant's access's place.

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

The files and directories behind a filesystem Grant. One may wrap another and
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

Move a file or directory to another path.

```rust
pub trait Filesystem {
    fn rename(&self, from: &str, to: &str) -> Result<(), FilesystemError>;
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

## `OpenOptions`

How a file is opened: for reading, writing, creating, or truncating.

```rust
pub struct OpenOptions {}
```

## `Kind`

What a path names: a file or a directory.

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
