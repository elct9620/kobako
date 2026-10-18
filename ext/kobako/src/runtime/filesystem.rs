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

use kobako_runtime::filesystem::{Directory, Filesystem, Memory, Overlay, Temporary};
use kobako_runtime::grant::Grant;
use kobako_runtime::mount::{Access, Mount, Policy};

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
    temporary.define_singleton_method("new", function!(temporary_new, 0))?;
    let directory = module.define_class("Directory", ruby.class_object())?;
    directory.define_singleton_method("new", function!(directory_new, 1))?;
    let overlay = module.define_class("Overlay", ruby.class_object())?;
    overlay.define_singleton_method("new", function!(overlay_new, 0))?;
    overlay.define_method("layer", method!(overlay_layer, -1))?;
    Ok(())
}

/// `Memory.new(files = {})`: each `path => contents` pair becomes a file,
/// its directories created along the way.
fn memory_new(args: &[Value]) -> Result<MemoryFs, MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    let mut files = Vec::new();
    match args {
        [] => {}
        [given] => RHash::try_convert(*given)?.foreach(|path: String, contents: RString| {
            files.push((path, super::rstring_to_vec(contents)));
            Ok(magnus::r_hash::ForEach::Continue)
        })?,
        _ => {
            return Err(MagnusError::new(
                ruby.exception_arg_error(),
                format!(
                    "wrong number of arguments (given {}, expected 0..1)",
                    args.len()
                ),
            ))
        }
    }
    let memory: Memory = files.into_iter().collect();
    Ok(MemoryFs(Arc::new(memory)))
}

fn temporary_new() -> TemporaryFs {
    TemporaryFs(Arc::new(Temporary::new()))
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
        overlay: Overlay::new(),
        layers: Vec::new(),
    }
}

/// `layer(filesystem, access: :read)`, a new Overlay with `filesystem`
/// beneath this one's Layers.
fn overlay_layer(overlay: &OverlayFs, args: &[Value]) -> Result<OverlayFs, MagnusError> {
    let args = scan_args::<(Value,), (), (), (), RHash, ()>(args)?;
    let kwargs = get_kwargs::<_, (), (Option<Symbol>,), ()>(args.keywords, &[], &["access"])?;
    let (filesystem,) = args.required;
    let access = match kwargs.optional.0 {
        Some(access) => access_of(access)?,
        None => Access::Read,
    };
    let mut layers = overlay.layers.clone();
    layers.push(Opaque::from(filesystem));
    Ok(OverlayFs {
        overlay: overlay
            .overlay
            .clone()
            .layer(filesystem_of(filesystem)?, access),
        layers,
    })
}

fn access_of(access: Symbol) -> Result<Access, MagnusError> {
    match access.name()?.as_ref() {
        "read" => Ok(Access::Read),
        "read_write" => Ok(Access::ReadWrite),
        other => Err(MagnusError::new(
            Ruby::get().expect("Ruby thread").exception_arg_error(),
            format!("access must be :read or :read_write, got :{other}"),
        )),
    }
}

/// The Filesystem a Ruby handle of any kobako kind stands for.
fn filesystem_of(value: Value) -> Result<Arc<dyn Filesystem>, MagnusError> {
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
    if let Some(fs) = RubyFilesystem::of(value) {
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

/// A mount's block, asked in its access's place. A block that raises
/// refuses, so the guest's open fails and its script goes on.
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
                .unwrap_or(false)
        })
    }
}

/// The Grants one invocation carries, from the Sandbox's Mounts as
/// `[at, access, filesystem, block]` entries. Each block stays rooted on
/// the Ruby side for the synchronous call that hands it over.
pub(super) fn grants_from(mounts: RArray) -> Result<Vec<Grant>, MagnusError> {
    let mut grants = Vec::with_capacity(mounts.len());
    for index in 0..mounts.len() as isize {
        let entry: RArray = mounts.entry(index)?;
        let at: String = entry.entry(0)?;
        let access = access_of(entry.entry(1)?)?;
        let mount = Mount::new(at, filesystem_of(entry.entry(2)?)?, access);
        let block: Value = entry.entry(3)?;
        grants.push(Grant::Mount(if block.is_nil() {
            mount
        } else {
            mount.with_policy(Arc::new(RubyPolicy {
                block: Opaque::from(block),
            }))
        }));
    }
    Ok(grants)
}
