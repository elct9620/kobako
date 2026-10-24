//! The Ruby face of the Mount: `Kobako::Filesystem::Memory`,
//! `Temporary`, `Directory`, and `Overlay`, each standing for the
//! `kobako_runtime` Filesystem of that kind, and the per-invocation
//! translation of a Sandbox's Mounts into contract Grants.
//!
//! A kind kobako ships runs in Rust; Ruby only picks it and the access.
//! Ruby is asked again while the guest runs through a mount's block, once
//! per open or change, and through a Filesystem written in Ruby, once per
//! call.

mod ruby;

use std::sync::Arc;

use magnus::value::{Opaque, ReprValue};
use magnus::{
    function, gc, method, prelude::*, scan_args::get_kwargs, scan_args::scan_args,
    DataTypeFunctions, Error as MagnusError, RArray, RHash, RModule, RString, Ruby, Symbol, Value,
};

use ruby::RubyFilesystem;

use super::pinned_value::PinnedValue;

use kobako_runtime::filesystem::{
    Access, Blocking, Directory, Filesystem, Memory, Overlay, Temporary,
};
use kobako_runtime::grant::Grant;
use kobako_runtime::mount::{Mount, Policy};

macro_rules! filesystem_class {
    ($name:ident, $class:literal) => {
        #[magnus::wrap(class = $class, free_immediately, size)]
        pub(super) struct $name(Arc<dyn Filesystem>);
    };
}

filesystem_class!(MemoryFs, "Kobako::Filesystem::Memory");
filesystem_class!(TemporaryFs, "Kobako::Filesystem::Temporary");
filesystem_class!(DirectoryFs, "Kobako::Filesystem::Directory");

/// The Overlay itself rather than a shared handle, so `layer` can answer a
/// new one and leave this one as it was. It marks the Ruby object of
/// every Layer, since a Filesystem written in Ruby has nothing else
/// keeping it alive.
#[derive(magnus::TypedData)]
#[magnus(class = "Kobako::Filesystem::Overlay", free_immediately, size, mark)]
pub(super) struct OverlayFs {
    overlay: Overlay,
    layers: Vec<Opaque<Value>>,
}

impl DataTypeFunctions for OverlayFs {
    fn mark(&self, marker: &gc::Marker) {
        marker.mark_slice(&self.layers);
    }
}

pub(super) fn register(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    let module = kobako.define_module("Filesystem")?;
    let memory = module.define_class("Memory", ruby.class_object())?;
    memory.define_singleton_method("new", function!(memory_new, -1))?;
    let temporary = module.define_class("Temporary", ruby.class_object())?;
    temporary.define_singleton_method("new", function!(temporary_new, -1))?;
    let directory = module.define_class("Directory", ruby.class_object())?;
    directory.define_singleton_method("new", function!(directory_new, 1))?;
    let overlay = module.define_class("Overlay", ruby.class_object())?;
    overlay.define_singleton_method("new", function!(overlay_new, 0))?;
    overlay.define_method("layer", method!(overlay_layer, -1))?;
    Ok(())
}

/// Refuse a Mount at `mount` time by the same rules an invocation builds
/// it with, so a mistake raises where it was made.
pub(super) fn validate_mount(
    at: Value,
    access: Value,
    filesystem: Value,
) -> Result<(), MagnusError> {
    to_mount(at, access, filesystem).map(drop)
}

/// The Mount a Sandbox's `[at, access, filesystem]` stands for: one owner
/// of the path, access, and Filesystem rules.
fn to_mount(at: Value, access: Value, filesystem: Value) -> Result<Mount, MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    let at = RString::try_convert(at)?.to_string()?;
    let access = to_access(access)?;
    Mount::new(at, to_filesystem(filesystem)?, access)
        .map_err(|err| MagnusError::new(ruby.exception_arg_error(), err.to_string()))
}

/// `Memory.new(files = {}, limit: 64 MiB, inodes:)`: each
/// `path => contents` pair becomes a file, its directories created along
/// the way. Ruby hands a bare `"path" => contents` over as keywords, so
/// every String keyword is a file too. Without `inodes:` the entries
/// follow the limit.
fn memory_new(args: &[Value]) -> Result<MemoryFs, MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    let args = scan_args::<(), (Option<RHash>,), (), (), RHash, ()>(args)?;
    let mut files = Vec::new();
    let mut limit = Some(Memory::DEFAULT_LIMIT);
    let mut inodes = None;
    let limit_key = ruby.to_symbol("limit");
    let inodes_key = ruby.to_symbol("inodes");
    for hash in args.optional.0.into_iter().chain([args.keywords]) {
        hash.foreach(|key: Value, value: Value| {
            match Symbol::from_value(key) {
                Some(key) if key.equal(limit_key)? => limit = to_limit(value)?,
                Some(key) if key.equal(inodes_key)? => inodes = Some(to_limit(value)?),
                Some(key) => {
                    return Err(MagnusError::new(
                        ruby.exception_arg_error(),
                        format!("unknown keyword: :{}", key.name()?),
                    ))
                }
                None => files.push((
                    String::try_convert(key)?,
                    super::rstring_to_vec(RString::try_convert(value)?),
                )),
            }
            Ok(magnus::r_hash::ForEach::Continue)
        })?;
    }
    // A list no Memory holds is an `ArgumentError` the Host App can rescue.
    let memory = Memory::try_from_iter(files)
        .map_err(|error| MagnusError::new(ruby.exception_arg_error(), error.to_string()))?
        .with_limit(limit);
    Ok(MemoryFs(Arc::new(match inodes {
        Some(inodes) => memory.with_inodes(inodes),
        None => memory,
    })))
}

/// `Temporary.new(limit: 64 MiB, inodes:)`.
fn temporary_new(args: &[Value]) -> Result<TemporaryFs, MagnusError> {
    let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
    let kwargs = get_kwargs::<_, (), (Option<Value>, Option<Value>), ()>(
        args.keywords,
        &[],
        &["limit", "inodes"],
    )?;
    let (limit, inodes) = kwargs.optional;
    let limit = match limit {
        Some(limit) => to_limit(limit)?,
        None => Some(Memory::DEFAULT_LIMIT),
    };
    let temporary = Temporary::new().with_limit(limit);
    Ok(TemporaryFs(Arc::new(match inodes {
        Some(inodes) => temporary.with_inodes(to_limit(inodes)?),
        None => temporary,
    })))
}

/// A `limit:` or `inodes:` count, or `nil` for none.
fn to_limit(limit: Value) -> Result<Option<usize>, MagnusError> {
    Option::<usize>::try_convert(limit)
}

fn directory_new(path: String) -> Result<DirectoryFs, MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    Directory::new(&path)
        .map(|directory| DirectoryFs(Arc::new(directory)))
        .map_err(|err| {
            MagnusError::new(
                ruby.exception_arg_error(),
                format!("cannot open host directory {path}: {err}"),
            )
        })
}

/// `Overlay.new`, with no Layer yet.
fn overlay_new() -> OverlayFs {
    OverlayFs {
        overlay: Overlay::new().with_blocking(Arc::new(ReleasesGvl)),
        layers: Vec::new(),
    }
}

/// An Overlay's thread waits for its lock without the GVL, so a holder
/// asking a Layer written in Ruby can take the GVL and finish.
struct ReleasesGvl;

impl Blocking for ReleasesGvl {
    fn wait(&self, wait: &mut dyn FnMut()) {
        super::gvl::region(super::gvl::holds(), wait);
    }
}

/// `layer(filesystem, access: :read)`, a new Overlay with `filesystem`
/// beneath this one's Layers.
fn overlay_layer(overlay: &OverlayFs, args: &[Value]) -> Result<OverlayFs, MagnusError> {
    let args = scan_args::<(Value,), (), (), (), RHash, ()>(args)?;
    let kwargs = get_kwargs::<_, (), (Option<Value>,), ()>(args.keywords, &[], &["access"])?;
    let (filesystem,) = args.required;
    let access = match kwargs.optional.0 {
        Some(access) => to_access(access)?,
        None => Access::Read,
    };
    let mut layers = overlay.layers.clone();
    layers.push(Opaque::from(filesystem));
    Ok(OverlayFs {
        overlay: overlay
            .overlay
            .clone()
            .layer(to_filesystem(filesystem)?, access),
        layers,
    })
}

fn to_access(access: Value) -> Result<Access, MagnusError> {
    let name = Symbol::from_value(access)
        .map(|symbol| symbol.name())
        .transpose()?;
    match name.as_deref() {
        Some("read") => Ok(Access::Read),
        Some("read_write") => Ok(Access::ReadWrite),
        _ => Err(MagnusError::new(
            Ruby::get().expect("Ruby thread").exception_arg_error(),
            format!(
                "access must be :read or :read_write, got {}",
                access.inspect()
            ),
        )),
    }
}

/// The Filesystem a Ruby handle of any kobako kind stands for.
fn to_filesystem(value: Value) -> Result<Arc<dyn Filesystem>, MagnusError> {
    if let Ok(fs) = <&MemoryFs>::try_convert(value) {
        return Ok(fs.0.clone());
    }
    if let Ok(fs) = <&TemporaryFs>::try_convert(value) {
        return Ok(fs.0.clone());
    }
    if let Ok(fs) = <&DirectoryFs>::try_convert(value) {
        return Ok(fs.0.clone());
    }
    if let Ok(fs) = <&OverlayFs>::try_convert(value) {
        return Ok(Arc::new(fs.overlay.clone()));
    }
    if let Some(fs) = RubyFilesystem::from_value(value) {
        return Ok(Arc::new(fs));
    }
    let ruby = Ruby::get().expect("Ruby thread");
    Err(MagnusError::new(
        ruby.exception_type_error(),
        format!(
            "expected a Kobako::Filesystem or an object answering open and stat, got {}",
            value.class()
        ),
    ))
}

/// A mount's block, asked in its access's place. A block raising a
/// StandardError refuses, so the guest's open fails and its script goes
/// on; anything else it raises is held for the host.
struct RubyPolicy {
    block: Opaque<Value>,
}

impl Policy for RubyPolicy {
    fn allows(&self, path: &str, access: Access) -> bool {
        super::gvl::reenter(|| {
            let ruby = Ruby::get().expect("Ruby handle unavailable in a mount's block");
            let block: Value = ruby.get_inner(self.block);
            let access = ruby.to_symbol(match access {
                Access::Read => "read",
                Access::ReadWrite => "read_write",
            });
            block
                .funcall::<_, _, Value>("call", (ruby.str_new(path), access))
                .map(|answer| answer.to_bool())
                .unwrap_or_else(|error| {
                    let _ = super::escape::hold(&ruby, error);
                    false
                })
        })
    }
}

/// The Grants one invocation carries, from the Sandbox's Mounts as
/// `[at, access, filesystem, block]` entries, beside the pins keeping
/// each Mount's filesystem and block in place while the guest may reach
/// them; the caller holds both until the invocation returns.
pub(super) fn to_grants(
    ruby: &Ruby,
    mounts: RArray,
) -> Result<(Vec<Grant>, Vec<PinnedValue>), MagnusError> {
    let mut grants = Vec::with_capacity(mounts.len());
    let mut pins = Vec::with_capacity(mounts.len() * 2);
    for index in 0..mounts.len() as isize {
        let entry: RArray = mounts.entry(index)?;
        let filesystem: Value = entry.entry(2)?;
        pins.push(PinnedValue::new(ruby, filesystem));
        let mount = to_mount(entry.entry(0)?, entry.entry(1)?, filesystem)?;
        let block: Value = entry.entry(3)?;
        grants.push(Grant::Mount(if block.is_nil() {
            mount
        } else {
            pins.push(PinnedValue::new(ruby, block));
            mount.with_policy(Arc::new(RubyPolicy {
                block: Opaque::from(block),
            }))
        }));
    }
    Ok((grants, pins))
}
