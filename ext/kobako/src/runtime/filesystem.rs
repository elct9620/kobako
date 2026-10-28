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
    function, gc, method, prelude::*, scan_args::get_kwargs, scan_args::scan_args,
    DataTypeFunctions, Error as MagnusError, RHash, RModule, RString, Ruby, Symbol, TypedData,
    Value,
};

use ruby::RubyFilesystem;

use kobako_runtime::filesystem::{Access, Bind, Blocking, Inode, Overlay, Tmpfs};
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

/// The Overlay itself rather than a shared handle, so `layer` can answer a
/// new one and leave this one as it was. It marks the Ruby object of
/// every Layer, since a Filesystem written in Ruby has nothing else
/// keeping it alive.
#[derive(magnus::TypedData)]
#[magnus(class = "Kobako::Filesystem::Overlay", free_immediately, size, mark)]
pub(super) struct OverlayFs {
    overlay: Overlay,
    filesystem: Arc<Filesystem>,
    layers: Vec<Opaque<Value>>,
}

impl OverlayFs {
    fn new(overlay: Overlay, layers: Vec<Opaque<Value>>) -> Self {
        Self {
            filesystem: new_filesystem(Arc::new(overlay.clone())),
            overlay,
            layers,
        }
    }
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
    overlay.define_singleton_method("new", function!(overlay_new, 0))?;
    overlay.define_method("layer", method!(overlay_layer, -1))?;
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
    Ok(TmpfsFs(new_filesystem(Arc::new(match nr_inodes {
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
        .map(|directory| BindFs(new_filesystem(Arc::new(directory))))
        .map_err(|err| {
            MagnusError::new(
                ruby.exception_arg_error(),
                format!("cannot open host directory {path}: {err}"),
            )
        })
}

/// `Overlay.new`, with no Layer yet.
fn overlay_new() -> OverlayFs {
    OverlayFs::new(
        Overlay::new().with_blocking(Arc::new(GvlFreeWait)),
        Vec::new(),
    )
}

/// The Filesystem instance `root` answers for, its calls waiting for a
/// directory's lock without the GVL.
fn new_filesystem(root: Arc<dyn Inode>) -> Arc<Filesystem> {
    Filesystem::with_blocking(root, Arc::new(GvlFreeWait))
}

/// A thread waits for a lock without the GVL, so a holder asking a
/// Filesystem written in Ruby can take the GVL and finish.
struct GvlFreeWait;

impl Blocking for GvlFreeWait {
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
    Ok(OverlayFs::new(
        overlay
            .overlay
            .clone()
            .layer(to_filesystem(filesystem)?, access),
        layers,
    ))
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
        return Ok(new_filesystem(Arc::new(fs)));
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
