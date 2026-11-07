use std::sync::Arc;

use kobako_runtime::filesystem::Filesystem;
use kobako_runtime::grant::{Grant, Policy};
use kobako_runtime::mount::{Access, Mount, MountRequest};

use crate::catalog::Catalog;
use crate::dispatch::Resolutions;
use crate::error::Error;
use crate::grants::shares_key;
use crate::receiver::Receiver;

/// What one invocation's override closure chose: the Services it rebound
/// and the Grants it replaced.
pub(super) struct Overrides {
    pub(super) resolved: Resolutions,
    pub(super) grants: Vec<Grant>,
}

pub(super) fn collect_overrides<F>(catalog: &Catalog, overrides: F) -> Result<Overrides, Error>
where
    F: FnOnce(&mut Context<'_>) -> Result<(), Error>,
{
    let mut ctx = Context {
        catalog,
        overrides: Vec::new(),
        grants: Vec::new(),
    };
    overrides(&mut ctx)?;
    Ok(Overrides {
        resolved: ctx.overrides,
        grants: ctx.grants,
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
    grants: Vec<Grant>,
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
        policy: Arc<dyn for<'a> Policy<MountRequest<'a>>>,
    ) -> Result<(), Error> {
        self.replace_mount(Mount::new(at, filesystem, Access::ReadWrite)?.with_policy(policy))
    }

    fn replace_mount(&mut self, mount: Mount) -> Result<(), Error> {
        let at = mount.at().to_string();
        let grant = Grant::Mount(mount);
        if !self.catalog.grants.holds(&grant) {
            return Err(Error::Argument(format!(
                "cannot override unmounted path {at:?}"
            )));
        }
        self.grants.retain(|replaced| !shares_key(replaced, &grant));
        self.grants.push(grant);
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
