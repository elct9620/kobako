//! Kobako runtime — installs the Kobako module surface onto an mruby VM
//! and owns the class handles needed by the dispatch layer.
//!
//! ## Why a separate type from `beni::Mrb`
//!
//! `Mrb` is the language-level VM owner: it knows how to open and close
//! an mruby state and nothing about kobako's own object surface. The
//! kobako-specific registrations (`Kobako` module, the `Kobako::Transport`
//! namespace, the `Kobako::Proxy` capability module and the
//! `Kobako::Handle` proxy that includes it, `Kobako::ServiceError` /
//! `Kobako::Transport::Error`) belong to a different concern and live
//! behind this domain boundary. The IO / Kernel surface is the sibling
//! `kobako-io` crate's gem, composed alongside the bridge gem at
//! install time.
//!
//! The shape mirrors `magnus::Ruby` for CRuby: a value-type "token" that
//! proves you can talk to the runtime, with no Drop and no lifetime —
//! liveness is the caller's contract, just as it is for mruby's own C
//! API. The C-bridges in `crate::runtime::bridges` remain
//! `unsafe extern "C" fn` callbacks invoked by mruby, but their bodies
//! acquire a `Kobako` through `Kobako::resolve_raw` and then call
//! safe methods.
//!
//! ## Lifecycle
//!
//! `Kobako::init` is called once per `__kobako_eval` invocation,
//! immediately after `Mrb::open`. It registers every boot-time entity
//! and returns a `Kobako` carrying the resolved class handles. The
//! returned value is then used to drive the Frame 1 preamble through
//! `Kobako::install_bindings`.
//!
//! C-bridges enter on a raw `*mut mrb_state` — the
//! `beni::sys::mrb_func_t` ABI mandates it — but `beni::method!`
//! hands each body a borrowed `&Mrb`, which it passes to
//! `Kobako::resolve_raw` to obtain the same handle without repeating
//! registration.
//!
//! What this file holds is the install side: registering the surface and
//! raising through it. Reading a value out of the VM or building one into
//! it is `values`, which is also the surface a payload codec is handed.

pub(crate) mod block_stack;
pub(crate) mod bridges;
pub(crate) mod codec_slot;
mod init;
mod values;

pub use values::IntegerOutOfRange;

use beni::sys;
use beni::Mrb;

/// Failures returned by `Kobako::install_bindings` when a preamble entry
/// cannot be registered — a path segment that cannot pass through the
/// mruby C API (which expects NUL-terminated strings), or a registration
/// mruby itself rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Handle to a Kobako runtime installed on a live mruby VM.
///
/// `Kobako` is a value-type token: it carries the raw `*mut mrb_state`
/// pointer plus the resolved class handles, but does not own the VM —
/// the caller is responsible for keeping the underlying state live for
/// the duration of any `Kobako` method call. Constructed through one of
/// two entry points:
///
///   * `Kobako::init` — register every boot-time entity then
///     return a fully populated handle. Takes an `Mrb` borrow so the
///     pipeline below it stays in safe Rust.
///   * `Kobako::resolve_raw` — re-resolve class handles produced by
///     a prior init, taking the `&Mrb` that `beni::method!` hands a
///     C-bridge body. Stays `unsafe`: the returned token keeps a raw
///     pointer the caller must keep live past the borrow.
///
/// ## Two seams, one token
///
/// The public methods serve the two things a third party replaces, and
/// which group a method belongs to says who it is for:
///
///   * **The payload codec seam** (`PayloadCodec`) — `mrb`,
///     `mint_handle`, `extract_handle_id`, `narrow_int`. A codec walking
///     its own value tree needs all four: VM access, the Handle spelling,
///     and the integer-range guard.
///   * **The invocation flow seam** (`MrbGuest::run` and friends) —
///     `init`, `resolve_raw`, `install_bindings`, `top_level_constants`,
///     `extract_backtrace`, `set_handle_id`, `raise_transport_error`.
///     These look internal to the bundled flows, and are exactly what
///     someone writing their own flow reaches for.
///
/// ## Placeholder mode
///
/// The type and its methods compile on every target; without a
/// linked `libmruby.a` (host builds in beni placeholder mode) the
/// operations they delegate to panic at runtime — see the crate doc.
pub struct Kobako {
    mrb: *mut sys::mrb_state,
    /// `Kobako::Proxy` capability module — extended onto every bound
    /// constant installed via `Kobako::install_bindings`.
    proxy_module: beni::RModule,
    handle_class: beni::RClass,
    service_error_class: beni::RClass,
    transport_error_class: beni::RClass,
}

// The canonical mruby `nil` / `true` / `false` value snapshots no
// longer live on the `Kobako` struct. They are captured once into
// the sys-side `Value` immediates cache and read via
// `Value::nil()` / `Value::true_()` / `Value::false_()` — each call
// is a single atomic load against the `OnceLock`, on par with the
// previous per-instance field read.

impl Kobako {
    /// Install the Kobako runtime onto `mrb` — the built-in
    /// `KobakoBridge` gem (classes + C bridges, the precondition of
    /// `Kobako::resolve_raw`) followed by the shell-chosen gem set
    /// from `G`'s `init_gems` hook — and return a handle to the
    /// resulting class registrations. An `Err` means mruby rejected a
    /// boot-time registration; the boot path surfaces it as a Panic.
    pub fn init<G: crate::MrbGuest>(mrb: &Mrb) -> Result<Self, beni::Error> {
        // The dispatch bridge mruby calls is a bare function pointer, so it
        // reads the guest's codec from here rather than from `G`.
        codec_slot::install::<G::Codec>();
        mrb.init_gem::<init::KobakoBridge>()?;
        G::init_gems(mrb)?;

        // SAFETY: `KobakoBridge::init` just registered every entity
        // `resolve_raw` looks up, satisfying its init precondition; the
        // invocation VM behind `mrb` outlives the returned token.
        Ok(unsafe { Self::resolve_raw(mrb) })
    }

    /// Resolve the class handles produced by a prior init, from the
    /// `&Mrb` that `beni::method!` hands a C-bridge body — the way
    /// those bodies recover the `Kobako` handle.
    ///
    /// # Safety
    ///
    /// `Kobako::init` must already have run on the state behind `mrb`,
    /// and that state must outlive the returned token, which keeps a
    /// raw pointer to it with no lifetime binding. The C-bridge entry
    /// points uphold both by construction — they run on the live
    /// invocation VM through registrations done at init time. (Missing
    /// init does not corrupt: each `expect` below panics instead.)
    pub unsafe fn resolve_raw(mrb: &Mrb) -> Self {
        use beni::Module;

        // `mrb_define_module` is idempotent (returns the existing
        // module if already registered); each `class_get` returns the
        // already-registered class produced by `init`, so every
        // `expect` below is the init precondition restated.
        const INITIALIZED: &str = "Kobako::init registered this entity";
        let kobako_mod = mrb.define_module(c"Kobako").expect(INITIALIZED);
        let transport_mod = kobako_mod
            .define_module(mrb, c"Transport")
            .expect(INITIALIZED);
        let proxy_module = kobako_mod.define_module(mrb, c"Proxy").expect(INITIALIZED);
        let handle_class = kobako_mod.class_get(mrb, c"Handle").expect(INITIALIZED);
        let service_error_class = kobako_mod
            .class_get(mrb, c"ServiceError")
            .expect(INITIALIZED);
        let transport_error_class = transport_mod.class_get(mrb, c"Error").expect(INITIALIZED);
        Self {
            mrb: mrb.as_ptr(),
            proxy_module,
            handle_class,
            service_error_class,
            transport_error_class,
        }
    }

    /// Install a bound-constant proxy for each bind path from a Frame 1
    /// preamble: define a class at the path and `extend Kobako::Proxy`
    /// onto it, so a class-level call dispatches to the host. A
    /// multi-segment path nests the leaf class under a module per prefix
    /// segment — idempotent, so paths sharing a prefix share one module —
    /// while a single-segment path binds the class at top level. The host
    /// guarantees no path is a prefix of another, so a segment is never
    /// both a module and a leaf.
    pub fn install_bindings(&self, paths: &[String]) -> Result<(), InstallError> {
        use beni::Module;

        let mrb = self.mrb();
        let object_class = mrb.object_class();
        for path in paths {
            let Some((prefix, leaf)) = path.rsplit_once("::") else {
                let name =
                    std::ffi::CString::new(path.as_str()).map_err(|_| InstallError::NulInName)?;
                let class = mrb
                    .define_class(name.as_c_str(), object_class)
                    .map_err(|e| InstallError::Rejected(e.message(mrb)))?;
                self.extend_proxy(mrb, class)?;
                continue;
            };

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
            let leaf_cstr = std::ffi::CString::new(leaf).map_err(|_| InstallError::NulInName)?;
            let class = module
                .define_class(mrb, leaf_cstr.as_c_str(), object_class)
                .map_err(|e| InstallError::Rejected(e.message(mrb)))?;
            self.extend_proxy(mrb, class)?;
        }
        Ok(())
    }

    /// Extend `Kobako::Proxy` onto `class` — the mruby `extend`, so the
    /// module's forwarding seam lands as the class's singleton methods and
    /// a class-level call on the bound constant dispatches to the host.
    fn extend_proxy(&self, mrb: &Mrb, class: beni::RClass) -> Result<(), InstallError> {
        // SAFETY: `class` is a live handle from `define_class` on this VM;
        // `proxy_module` shares its representation with `RClass`, so
        // reifying it through `RClass::to_value` yields the module value
        // `extend` mixes in.
        let class_val = unsafe { class.to_value(mrb) };
        let proxy_val = unsafe { beni::RClass::from_raw(self.proxy_module.as_raw()).to_value(mrb) };
        class_val
            .funcall(mrb, c"extend", &[proxy_val])
            .map(|_| ())
            .map_err(|e| InstallError::Rejected(e.message(mrb)))
    }

    /// Raise `Kobako::Transport::Error` with `msg`. Diverges — `mrb_raise` does
    /// not return.
    ///
    /// # Safety
    ///
    /// Only callable from contexts that mruby may unwind from (C
    /// bridges, mrb_funcall handlers). Calling from arbitrary Rust code
    /// would jump through mruby's exception machinery in a way the Rust
    /// stack does not anticipate.
    pub unsafe fn raise_transport_error(&self, msg: &core::ffi::CStr) -> ! {
        // SAFETY: bridge frame — caller upholds the unwind contract.
        unsafe { self.transport_error_class.raise(self.mrb(), msg) };
    }

    /// Raise `Kobako::ServiceError` for `ex`. Diverges — `mrb_raise`
    /// does not return.
    ///
    /// SPEC.md § Error Classes (governing) + docs/wire-contract.md
    /// § Fault pin every fault-arm `type` value to the
    /// single guest-side `Kobako::ServiceError` class.
    ///
    /// # Safety
    ///
    /// As `Kobako::raise_transport_error`.
    pub(crate) unsafe fn raise_service_error(
        &self,
        fault: &kobako_transport::envelope::Fault,
    ) -> ! {
        let msg = std::ffi::CString::new(fault.message.as_str()).unwrap_or_default();
        // SAFETY: bridge frame — caller upholds the unwind contract.
        unsafe { self.service_error_class.raise(self.mrb(), &msg) };
    }

    // ----------------------------------------------------------------
    // VM access. The `mrb` accessor synthesises a borrowed `Mrb`
    // reference over the raw pointer so callers can use the safe
    // builder / accessor methods (`hash_get`, `intern_cstr`, etc.)
    // without each method re-implementing the same FFI dispatch.
    // ----------------------------------------------------------------

    /// Borrow `self.mrb` as `&Mrb`. The borrow lives for the duration
    /// of `&self`, which the `Kobako` construction contract ties
    /// to the underlying `mrb_state`'s liveness.
    #[inline]
    pub fn mrb(&self) -> &Mrb {
        // SAFETY: `Kobako` is only constructed against a live
        // `mrb_state` (via `init` / `resolve_raw`), and the caller
        // upholds liveness for the duration of any method call on it.
        unsafe { Mrb::borrow_raw(&self.mrb) }
    }
}
