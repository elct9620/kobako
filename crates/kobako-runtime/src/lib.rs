//! kobako-runtime — engine-neutral host runtime contract.
//!
//! The surface where a wasm engine implementation and a host frontend
//! meet: the `Runtime` trait, the isolation `Profile` a runtime
//! declares, the Grants it presents to the guest and the Filesystem a
//! Mount places, the neutral per-invocation value types, and the dispatch /
//! yield re-entry traits a frontend supplies. The messages these carry
//! are `kobako-transport`'s, which this crate takes as given.
//! Nothing here depends on an engine or a frontend type — each engine
//! hides its own machinery behind `Runtime`, and each frontend maps
//! these shapes onto its own host-language surface at its boundary
//! (for the Ruby ext that is the error mapper in its runtime module),
//! so the engine stays swappable.

pub mod dispatch;
pub mod endpoint;
pub mod error;
pub mod filesystem;
pub mod grant;
mod hashtable;
mod locking;
pub mod mount;
pub mod profile;
pub mod runtime;
pub mod sched;
pub mod security;
pub mod snapshot;
pub mod vfs;
pub mod yielder;

// Every name an engine implementation writes into its own signatures, at
// the crate root: the module path repeats the crate name and adds nothing
// a reader of `kobako_runtime::Runtime` was missing.
pub use dispatch::DispatchHandler;
pub use error::{InvokeError, SetupError, Trap};
pub use filesystem::{
    Bind, DentryOperations, DirEntry, FileOperations, FileType, FilesystemError, Iattr,
    InodeOperations, Kiocb, Lookup, Metadata, OpenOptions, Overlay, SeedError, Tmpfs,
};
pub use grant::{Grant, GrantKind};
pub use mount::{Access, Denial, Mount, MountPathError, Policy};
pub use profile::Profile;
pub use runtime::{Entry, Runtime};
pub use sched::Blocking;
pub use snapshot::{Capture, Completion, Snapshot, Usage};
pub use yielder::Yielder;
