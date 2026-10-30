use std::sync::Arc;

use kobako_runtime::mount::{Access, Mount, Policy};
use kobako_runtime::vfs::Filesystem;

use crate::catalog::Catalog;
use crate::dispatch::Resolutions;
use crate::error::Error;
use crate::receiver::Receiver;

/// What one invocation's override closure chose: the Services it rebound
/// and the Mounts it replaced.
pub(super) struct Overrides {
    pub(super) resolved: Resolutions,
    pub(super) mounts: Vec<Mount>,
}

pub(super) fn collect_overrides<F>(catalog: &Catalog, overrides: F) -> Result<Overrides, Error>
where
    F: FnOnce(&mut Context<'_>) -> Result<(), Error>,
{
    let mut ctx = Context {
        catalog,
        overrides: Vec::new(),
        mounts: Vec::new(),
    };
    overrides(&mut ctx)?;
    Ok(Overrides {
        resolved: ctx.overrides,
        mounts: ctx.mounts,
    })
}

/// The per-invocation Context handed to an `eval_with` / `run_with` override closure — the
/// Rust peer of the Ruby frontend's `Kobako::Context`. Here it carries only the
/// `ctx.bind` and `ctx.mount` overrides; the run's Handle table and observables ride into its
/// `Execution` instead. Each override takes priority over that path's
/// per-invocation provider result and its static base for that one invocation,
/// and is discarded when it ends. The borrow keeps it from outliving the
/// closure, which is what spends it.
pub struct Context<'a> {
    catalog: &'a Catalog,
    overrides: Resolutions,
    mounts: Vec<Mount>,
}

impl Context<'_> {
    /// Use `filesystem` for the Mount at `at` during this invocation only,
    /// with its own `access`. Replacing a path nothing was mounted at returns
    /// `Error::Argument`, so an override never adds a Mount.
    pub fn mount(
        &mut self,
        at: &str,
        filesystem: Arc<Filesystem>,
        access: Access,
    ) -> Result<(), Error> {
        self.replace_mount(Mount::new(at, filesystem, access)?)
    }

    /// `mount`, read-write, with a `policy` judging each call, as
    /// `Sandbox::mount_with_policy` takes it.
    pub fn mount_with_policy(
        &mut self,
        at: &str,
        filesystem: Arc<Filesystem>,
        policy: Arc<dyn Policy>,
    ) -> Result<(), Error> {
        self.replace_mount(Mount::new(at, filesystem, Access::ReadWrite)?.with_policy(policy))
    }

    fn replace_mount(&mut self, mount: Mount) -> Result<(), Error> {
        let at = mount.at();
        if !self.catalog.grants.is_mounted(at) {
            return Err(Error::Argument(format!(
                "cannot override unmounted path {at:?}"
            )));
        }
        self.mounts.retain(|mounted| mounted.at() != at);
        self.mounts.push(mount);
        Ok(())
    }

    /// Override the object bound at an already-declared `path` for this
    /// invocation. Overriding a path that was never declared returns
    /// `Error::Argument`, so an override can never grow the sealed key set;
    /// a second override of the same `path` wins over the first.
    pub fn bind(&mut self, path: &str, object: Arc<dyn Receiver>) -> Result<(), Error> {
        if self.catalog.lookup(path).is_none() {
            return Err(Error::Argument(format!(
                "cannot override undeclared path {path:?}"
            )));
        }
        self.overrides.retain(|(bound, _)| bound != path);
        self.overrides.push((path.to_string(), object));
        Ok(())
    }
}
