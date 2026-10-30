//! The Ruby face of the Filesystem: `Kobako::Filesystem::Tmpfs`,
//! `Bind`, and `Overlay`, each standing for the `kobako_runtime`
//! Filesystem of that kind.
//!
//! A kind kobako ships runs in Rust; Ruby only picks it and the access.
//! Ruby is asked again while the guest runs through a mount's block, at
//! most once per open or change, and through a Filesystem written in
//! Ruby, once per call.

mod grants;
mod ruby;

use std::sync::Arc;

use magnus::value::{Opaque, ReprValue};
use magnus::{
    function, gc, prelude::*, scan_args::get_kwargs, scan_args::scan_args, DataTypeFunctions,
    Error as MagnusError, RArray, RHash, RModule, RString, Ruby, Symbol, TypedData, Value,
};

use ruby::RubyFilesystem;

use kobako_runtime::filesystem::{Bind, Overlay, Tmpfs};
use kobako_runtime::vfs::Filesystem;

pub(super) use grants::{to_grants, validate_mount};

/// Each kind kobako ships is one Filesystem instance, made once with its
/// Ruby object, so every Mount of that object shares it as a bind mount
/// shares a super block.
macro_rules! filesystem_class {
    ($name:ident, $class:literal) => {
        #[magnus::wrap(class = $class, free_immediately, size)]
        pub(super) struct $name(Arc<Filesystem>);
    };
}

filesystem_class!(TmpfsFs, "Kobako::Filesystem::Tmpfs");
filesystem_class!(BindFs, "Kobako::Filesystem::Bind");

/// The Overlay's one Filesystem instance. It marks the Ruby object of
/// every Layer, since a Filesystem written in Ruby has nothing else
/// keeping it alive.
#[derive(magnus::TypedData)]
#[magnus(class = "Kobako::Filesystem::Overlay", free_immediately, size, mark)]
pub(super) struct OverlayFs {
    filesystem: Arc<Filesystem>,
    layers: Vec<Opaque<Value>>,
}

impl DataTypeFunctions for OverlayFs {
    fn mark(&self, marker: &gc::Marker) {
        marker.mark_slice(&self.layers);
    }
}

pub(super) fn register(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    let module = kobako.define_module("Filesystem")?;
    let tmpfs = module.define_class("Tmpfs", ruby.class_object())?;
    tmpfs.define_singleton_method("new", function!(tmpfs_new, -1))?;
    let bind = module.define_class("Bind", ruby.class_object())?;
    bind.define_singleton_method("new", function!(bind_new, 1))?;
    let overlay = module.define_class("Overlay", ruby.class_object())?;
    overlay.define_singleton_method("new", function!(overlay_new, -1))?;
    TmpfsFs::class(ruby);
    BindFs::class(ruby);
    OverlayFs::class(ruby);
    Ok(())
}

/// `Tmpfs.new(files = {}, size: 64 MiB, nr_inodes:)`: each
/// `path => contents` pair becomes a file, its directories created along
/// the way. Ruby hands a bare `"path" => contents` over as keywords, so
/// every String keyword is a file too. Without `nr_inodes:` the entries
/// follow the size.
fn tmpfs_new(args: &[Value]) -> Result<TmpfsFs, MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    let args = scan_args::<(), (Option<RHash>,), (), (), RHash, ()>(args)?;
    let mut files = Vec::new();
    let mut size = Some(Tmpfs::DEFAULT_SIZE);
    let mut nr_inodes = None;
    let size_key = ruby.to_symbol("size");
    let nr_inodes_key = ruby.to_symbol("nr_inodes");
    for hash in args.optional.0.into_iter().chain([args.keywords]) {
        hash.foreach(|key: Value, value: Value| {
            match Symbol::from_value(key) {
                Some(key) if key.equal(size_key)? => size = to_count(value)?,
                Some(key) if key.equal(nr_inodes_key)? => nr_inodes = Some(to_count(value)?),
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
    // A list no Tmpfs holds is an `ArgumentError` the Host App can rescue.
    let tmpfs = Tmpfs::try_from_iter(files)
        .map_err(|error| MagnusError::new(ruby.exception_arg_error(), error.to_string()))?
        .with_size(size);
    Ok(TmpfsFs(Filesystem::new(Arc::new(match nr_inodes {
        Some(nr_inodes) => tmpfs.with_nr_inodes(nr_inodes),
        None => tmpfs,
    }))))
}

/// A `size:` or `nr_inodes:` count, or `nil` for none.
fn to_count(count: Value) -> Result<Option<usize>, MagnusError> {
    Option::<usize>::try_convert(count)
}

fn bind_new(path: String) -> Result<BindFs, MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    Bind::new(&path)
        .map(|directory| BindFs(Filesystem::new(Arc::new(directory))))
        .map_err(|err| {
            MagnusError::new(
                ruby.exception_arg_error(),
                format!("cannot open host directory {path}: {err}"),
            )
        })
}

/// `Overlay.new(lower:, upper: nil)`: the Filesystems in `lower`, topmost
/// first, under `upper`, a Tmpfs. Without `upper` it takes two lower
/// Layers at least.
fn overlay_new(args: &[Value]) -> Result<OverlayFs, MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
    let kwargs =
        get_kwargs::<_, (RArray,), (Option<Value>,), ()>(args.keywords, &["lower"], &["upper"])?;
    let lower = (0..kwargs.required.0.len())
        .map(|index| kwargs.required.0.entry::<Value>(index as isize))
        .collect::<Result<Vec<_>, _>>()?;
    let upper = kwargs.optional.0.filter(|upper| !upper.is_nil());
    let upper_fs = match upper {
        Some(upper) => match <&TmpfsFs>::try_convert(upper) {
            Ok(tmpfs) => Some(tmpfs.0.clone()),
            Err(_) => {
                return Err(MagnusError::new(
                    ruby.exception_type_error(),
                    format!(
                        "upper must be a Kobako::Filesystem::Tmpfs, got {}",
                        upper.class()
                    ),
                ))
            }
        },
        None => None,
    };
    let lower_fs = lower
        .iter()
        .map(|layer| to_filesystem(*layer))
        .collect::<Result<Vec<_>, _>>()?;
    let overlay = Overlay::new(lower_fs, upper_fs).map_err(|_| {
        MagnusError::new(
            ruby.exception_arg_error(),
            "an Overlay needs a lower Layer under its upper, or two lower Layers without one",
        )
    })?;
    Ok(OverlayFs {
        filesystem: Filesystem::new(Arc::new(overlay)),
        layers: lower.into_iter().chain(upper).map(Opaque::from).collect(),
    })
}

/// The Filesystem instance a Ruby handle of any kobako kind stands for. A
/// Filesystem written in Ruby has no instance of its own to share, so each
/// call makes one.
fn to_filesystem(value: Value) -> Result<Arc<Filesystem>, MagnusError> {
    if let Ok(fs) = <&TmpfsFs>::try_convert(value) {
        return Ok(fs.0.clone());
    }
    if let Ok(fs) = <&BindFs>::try_convert(value) {
        return Ok(fs.0.clone());
    }
    if let Ok(fs) = <&OverlayFs>::try_convert(value) {
        return Ok(fs.filesystem.clone());
    }
    if let Some(fs) = RubyFilesystem::from_value(value) {
        return Ok(Filesystem::new(Arc::new(fs)));
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
