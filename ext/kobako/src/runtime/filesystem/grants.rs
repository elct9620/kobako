//! A Sandbox's Mounts as contract Grants: refused at `mount` time and
//! built afresh for every invocation by the same rules, each mount's
//! block standing in for its access.

use std::sync::Arc;

use magnus::value::{Opaque, ReprValue};
use magnus::{prelude::*, Error as MagnusError, RArray, RString, Ruby, Symbol, Value};

use kobako_runtime::grant::Grant;
use kobako_runtime::mount::{Access, Mount, Policy};

use super::to_filesystem;
use crate::runtime::pinned_value::PinnedValue;
use crate::runtime::{escape, gvl};

/// Refuse a Mount at `mount` time by the same rules an invocation builds
/// it with, so a mistake raises where it was made.
pub(in crate::runtime) fn validate_mount(
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

/// A mount's block, asked what its access allows. A block raising a
/// StandardError refuses, so the guest's open fails and its script goes
/// on; anything else it raises is held for the host.
struct RubyPolicy {
    block: Opaque<Value>,
}

impl Policy for RubyPolicy {
    fn allows(&self, path: &str, access: Access) -> bool {
        gvl::reenter(|| {
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
                    let _ = escape::hold(&ruby, error);
                    false
                })
        })
    }
}

/// The Grants one invocation carries, from the Sandbox's Mounts as
/// `[at, access, filesystem, block]` entries, beside the pins keeping
/// each Mount's filesystem and block in place while the guest may reach
/// them; the caller holds both until the invocation returns.
pub(in crate::runtime) fn to_grants(
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
