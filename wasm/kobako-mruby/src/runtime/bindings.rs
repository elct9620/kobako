//! Installing the Frame 1 bind paths: one proxy class per path, nested
//! under a module per prefix segment.

use beni::Mrb;

use super::Kobako;

/// Failures returned by `Kobako::install_bindings` when a preamble entry
/// cannot be registered — a path segment that cannot pass through the
/// mruby C API (which expects NUL-terminated strings), or a registration
/// mruby itself rejected.
///
/// Non-exhaustive: a flow of one's own matches this to word its own boot
/// failure, and a later way registration can fail must not break the
/// wordings already written.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstallError {
    /// A bind path segment contained an interior NUL byte.
    NulInName,
    /// mruby rejected the module / class registration (e.g. a name
    /// that is not a valid constant); carries the rendered exception
    /// message.
    Rejected(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::NulInName => {
                f.write_str("bind path segment contains an invalid character")
            }
            InstallError::Rejected(msg) => {
                write!(f, "bind path registration rejected: {msg}")
            }
        }
    }
}

impl std::error::Error for InstallError {}

impl Kobako {
    /// Define a class at each bind path with `Kobako::Proxy` extended onto
    /// it, so a class-level call dispatches to the host. The host
    /// guarantees no path is a prefix of another, so a segment is never
    /// both a module and a leaf.
    pub fn install_bindings(&self, paths: &[String]) -> Result<(), InstallError> {
        use beni::Module;

        let mrb = self.mrb();
        let object_class = mrb.object_class();
        // Namespaces this call has already registered. Paths gathered under
        // one namespace are the ordinary registry shape, so resolving each
        // prefix once is what keeps a nested binding close to the price of
        // a top-level one.
        let mut namespaces: Vec<(&str, beni::RModule)> = Vec::new();
        for path in paths {
            let class = match path.rsplit_once("::") {
                None => {
                    let name = std::ffi::CString::new(path.as_str())
                        .map_err(|_| InstallError::NulInName)?;
                    mrb.define_class(name.as_c_str(), object_class)
                        .map_err(|e| InstallError::Rejected(e.message(mrb)))?
                }
                Some((prefix, leaf)) => {
                    let module = match namespaces.iter().find(|(seen, _)| *seen == prefix) {
                        Some(&(_, module)) => module,
                        None => {
                            let module = self.define_namespace(mrb, prefix)?;
                            namespaces.push((prefix, module));
                            module
                        }
                    };
                    let leaf_cstr =
                        std::ffi::CString::new(leaf).map_err(|_| InstallError::NulInName)?;
                    module
                        .define_class(mrb, leaf_cstr.as_c_str(), object_class)
                        .map_err(|e| InstallError::Rejected(e.message(mrb)))?
                }
            };
            self.extend_proxy(mrb, class)?;
        }
        Ok(())
    }

    fn define_namespace(&self, mrb: &Mrb, prefix: &str) -> Result<beni::RModule, InstallError> {
        use beni::Module;

        let mut segments = prefix.split("::");
        let first = segments.next().expect("split yields at least one segment");
        let first_cstr = std::ffi::CString::new(first).map_err(|_| InstallError::NulInName)?;
        let mut module = mrb
            .define_module(first_cstr.as_c_str())
            .map_err(|e| InstallError::Rejected(e.message(mrb)))?;
        for segment in segments {
            let segment_cstr =
                std::ffi::CString::new(segment).map_err(|_| InstallError::NulInName)?;
            module = module
                .define_module(mrb, segment_cstr.as_c_str())
                .map_err(|e| InstallError::Rejected(e.message(mrb)))?;
        }
        Ok(module)
    }

    fn extend_proxy(&self, mrb: &Mrb, class: beni::RClass) -> Result<(), InstallError> {
        use beni::{Module, Object};

        class
            .singleton_class(mrb)
            .and_then(|singleton| singleton.include_module(mrb, self.registrations.proxy_module))
            .map_err(|e| InstallError::Rejected(e.message(mrb)))
    }
}
