//! A Sandbox's Mounts as contract Grants: refused at `mount` time and
//! built afresh for every invocation by the same rules, each mount's
//! block standing in for its access.

use std::sync::Arc;

use magnus::value::{Opaque, ReprValue};
use magnus::{prelude::*, Error as MagnusError, RArray, RString, Ruby, Symbol, Value};

use kobako_runtime::grant::Grant;
use kobako_runtime::mount::{Access, Mount, MountRequest};
use kobako_runtime::policy::Policy;

use super::to_filesystem;
use crate::pinned_value::PinnedValue;
use crate::{escape, gvl, name_of, named, symbol_name_of};

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

/// Each access beside the name the `access:` option takes it by.
const ACCESS_NAMES: [(Access, &str); 2] =
    [(Access::Read, "read"), (Access::ReadWrite, "read_write")];

fn to_access(access: Value) -> Result<Access, MagnusError> {
    symbol_name_of(access)?
        .and_then(|name| named(&ACCESS_NAMES, &name))
        .ok_or_else(|| {
            MagnusError::new(
                Ruby::get().expect("Ruby thread").exception_arg_error(),
                format!(
                    "access must be :read or :read_write, got {}",
                    access.inspect()
                ),
            )
        })
}

/// `access` as the Symbol the `access:` option names it by.
fn to_access_name(ruby: &Ruby, access: Access) -> Symbol {
    ruby.to_symbol(name_of(&ACCESS_NAMES, &access).expect("every access has a name"))
}

/// A mount's block, asked what its access allows. A block raising a
/// StandardError refuses, so the guest's open fails and its script goes
/// on; anything else it raises is held for the host.
struct RubyPolicy {
    block: Opaque<Value>,
}

impl Policy<MountRequest<'_>> for RubyPolicy {
    fn allows(&self, request: &MountRequest<'_>) -> bool {
        gvl::reenter(|| {
            let ruby = Ruby::get().expect("Ruby handle unavailable in a mount's block");
            let block: Value = ruby.get_inner(self.block);
            let access = to_access_name(&ruby, request.access);
            block
                .funcall::<_, _, Value>("call", (ruby.str_new(request.path), access))
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
pub(crate) fn to_grants(
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
