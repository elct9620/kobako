//! The magnus bridge for the guest→host dispatch seam.
//!
//! The Ruby-Proc `DispatchHandler` and the frame-scoped `GuestYielder`
//! handle the Proc re-enters the guest through — the one place the
//! dispatch seam touches `magnus`. The wasm-side dispatch path
//! (`kobako_wasmtime`'s dispatch module) sees only the contract traits.

use core::cell::Cell;
use core::ptr::NonNull;

use magnus::value::{Opaque, ReprValue};
use magnus::{method, prelude::*, Error as MagnusError, RClass, RString, Ruby, Symbol, Value};

use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::yielder::Yielder;
use kobako_transport::envelope::{Call, Reply, Target, YieldReply};

/// The names the host `Transport::Yielder` branches on — one per Yield
/// Reply arm. Neutral Symbols rather than the envelope's tag bytes, the
/// same way `Snapshot#outcome` names its arms: the core envelope is this
/// side's to read, so no byte value of it reaches Ruby.
const ARM_OK: &str = "ok";
const ARM_BREAK: &str = "break";
const ARM_ERROR: &str = "error";

/// Register the `Kobako::Runtime::GuestYielder` Ruby class. Called from
/// `crate::runtime::init` after `Kobako::Runtime` is defined so the
/// `#[magnus::wrap]` class name resolves before any object is wrapped.
pub(super) fn register(runtime_class: RClass) -> Result<(), MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    let class = runtime_class.define_class("GuestYielder", ruby.class_object())?;
    class.define_method("call", method!(GuestYielder::call, 1))?;
    Ok(())
}

/// Frame-scoped Ruby handle that lets the dispatch `Proc` re-enter the
/// guest to run a yielded block. It wraps the active `&mut dyn Yielder`
/// for exactly one `__kobako_dispatch` frame: the bridge builds one, hands
/// it to the `Proc` as the second argument, and `invalidate`s it the
/// instant the `Proc` returns. A guest block stashed and called after that
/// frame normally raises `LocalJumpError` at the Ruby
/// `Transport::Yielder` net — invalidated in the dispatcher's `ensure`,
/// which fires before this handle is reached. This inner invalidation is
/// the backstop behind that outer net: it keeps `call`'s `unsafe`
/// `NonNull` deref from touching freed stack should the outer net ever be
/// bypassed, so neither net is redundant.
///
/// This is the single, explicit, frame-scoped FFI pointer the host↔guest
/// re-entry still costs: `magnus`' `funcall` sits between two Rust frames,
/// so the typed `&mut dyn Yielder` cannot cross it and is erased to a raw
/// pointer here. Unlike the dispatch `Proc`, this handle holds **no Ruby
/// `Value`**, so GC has nothing to trace through it — it needs no `mark`.
#[magnus::wrap(class = "Kobako::Runtime::GuestYielder", free_immediately, size)]
struct GuestYielder {
    yielder: Cell<Option<NonNull<dyn Yielder>>>,
}

// SAFETY: magnus requires `Send + Sync` on wrapped types. The raw pointer
// is created, used, and invalidated within a single `__kobako_dispatch`
// frame on the one Ruby thread that owns the active Invocation (SPEC.md
// Single-Invocation Slot); it is never read from another thread.
unsafe impl Send for GuestYielder {}
unsafe impl Sync for GuestYielder {}

impl GuestYielder {
    /// Erase the frame-scoped `&mut dyn Yielder` into a Ruby-owned handle.
    /// Safety contract for the caller: `invalidate` MUST run before the
    /// borrow this pointer came from ends (i.e. before the dispatch frame
    /// returns).
    fn new(yielder: &mut dyn Yielder) -> Self {
        let ptr = NonNull::from(yielder);
        // Erase the borrow's lifetime to `'static`; the pointer is only
        // ever dereferenced while it is still `Some` (i.e. `invalidate`
        // has not run), so the referent is guaranteed live.
        let ptr: NonNull<dyn Yielder> = unsafe {
            std::mem::transmute::<NonNull<dyn Yielder + '_>, NonNull<dyn Yielder + 'static>>(ptr)
        };
        Self {
            yielder: Cell::new(Some(ptr)),
        }
    }

    /// Mark this handle dead. Called the instant the dispatch frame's
    /// `funcall` returns, so a guest block stashed beyond its frame raises
    /// instead of dereferencing freed stack.
    fn invalidate(&self) {
        self.yielder.set(None);
    }

    /// Ruby-visible `call(args_payload) -> [arm, body, class]`: drive one
    /// yield round-trip and hand back the reply already split. The arm is
    /// named here off the core envelope, so the host `Transport::Yielder`
    /// decodes only a payload — and only on the two arms that carry one,
    /// where `body` is the block's value; on the error arm it is the
    /// message, and `class` names what raised.
    /// Raises `Kobako::TrapError` when the handle has been invalidated
    /// (escaped guest block), when the re-entry itself traps, or when the
    /// guest answers with bytes the envelope cannot frame.
    fn call(&self, args: RString) -> Result<(Symbol, RString, Option<String>), MagnusError> {
        let ruby = Ruby::get().expect("Ruby handle unavailable in __kobako_yield");
        let Some(mut ptr) = self.yielder.get() else {
            return Err(super::errors::trap_err(
                &ruby,
                "guest block invoked after the host dispatch frame returned",
            ));
        };
        let bytes = super::rstring_to_vec(args);
        // SAFETY: `yielder` is `Some`, so `invalidate` has not run — the
        // dispatch frame that lent the `&mut dyn Yielder` is still on the
        // Rust stack, and the Single-Invocation Slot guarantees no other
        // frame aliases it. The borrow ends with this method.
        let yielder: &mut dyn Yielder = unsafe { ptr.as_mut() };
        let resp = yielder
            .yield_block(&bytes)
            .map_err(|t| super::errors::trap_to_magnus(&ruby, t))?;
        match YieldReply::decode(&resp) {
            Ok(YieldReply::Ok(body)) => {
                Ok((ruby.to_symbol(ARM_OK), ruby.str_from_slice(&body), None))
            }
            Ok(YieldReply::Break(body)) => {
                Ok((ruby.to_symbol(ARM_BREAK), ruby.str_from_slice(&body), None))
            }
            Ok(YieldReply::Error(record)) => Ok((
                ruby.to_symbol(ARM_ERROR),
                ruby.str_new(&record.message),
                Some(record.class),
            )),
            Err(err) => Err(super::errors::trap_err(&ruby, err.to_string())),
        }
    }
}

/// The Ruby-Proc bridge: a `DispatchHandler` backed by the per-invocation
/// dispatch `Proc` handed to `Runtime#eval` / `#run` as a call argument.
/// This is the one place the dispatch seam touches `magnus`; the wasm
/// runtime sees only the trait. The Proc stays GC-rooted as a live argument
/// on the Ruby stack for the synchronous call; this struct holds an
/// `Opaque` copy of the same handle.
pub(super) struct RubyDispatchHandler {
    on_dispatch: Opaque<Value>,
}

impl RubyDispatchHandler {
    pub(super) fn new(on_dispatch: Opaque<Value>) -> Self {
        Self { on_dispatch }
    }
}

impl DispatchHandler for RubyDispatchHandler {
    /// Call the Ruby Proc with the routed Call and return its Reply. The
    /// envelope is already decoded, so Ruby receives the target, method,
    /// and block flag as ordinary values and decodes only the payload —
    /// through the MessagePack codec it already owns, which keeps a
    /// large payload's strings shared with the buffer rather than copied.
    ///
    /// The Proc is contracted to fold every dispatch failure into the
    /// Reply's fault arm (see `Kobako::Transport::Dispatcher.dispatch`),
    /// so a raise is a contract violation surfaced as `None` — the
    /// dispatcher then walks the 0-return wire-fault path.
    fn dispatch(&self, call: Call<'_>, yielder: &mut dyn Yielder) -> Option<Reply> {
        // The guest may be running GVL-free (`gvl: :release`); re-acquire the
        // GVL for the whole callback before touching any Ruby VALUE. In hold
        // mode `reenter` runs the body inline — the GVL is already held.
        super::gvl::reenter(|| {
            // The wasmtime callback runs on the same Ruby thread that called
            // the active Sandbox invocation (#eval or #run) — the invariant
            // SPEC Implementation Standards Architecture pins for the host gem
            // — so `Ruby::get()` is available here once the GVL is held.
            // Panicking with `expect` localises the violation rather than
            // letting a nonsense error propagate.
            let ruby = Ruby::get().expect("Ruby handle unavailable in __kobako_dispatch");
            let proc_value: Value = ruby.get_inner(self.on_dispatch);
            // The target crosses in one slot, already discriminated: a
            // String for a bound constant's path, an Integer for a Handle
            // id. Ruby branches on the class it received rather than on a
            // separate tag.
            let target: Value = match call.target {
                Target::Path(path) => ruby.str_new(path).as_value(),
                Target::Handle(id) => ruby.integer_from_u64(u64::from(id)).as_value(),
            };
            // Hand the Proc a frame-scoped yielder object as its last arg and
            // invalidate it the instant the Proc returns, so a guest block that
            // escapes the dispatch frame can never deref the freed stack
            // pointer. `guest_yielder` holds no Ruby Value, so it needs no GC
            // mark — the GC has nothing to trace through it.
            let guest_yielder = ruby.obj_wrap(GuestYielder::new(yielder));
            let answer: Result<(bool, RString), magnus::Error> = proc_value.funcall(
                "call",
                (
                    target,
                    ruby.str_new(call.method),
                    call.block_given,
                    ruby.str_from_slice(call.payload),
                    guest_yielder,
                ),
            );
            guest_yielder.invalidate();
            answer.ok().map(|(ok, body)| match ok {
                true => Reply::Ok(super::rstring_to_vec(body)),
                false => Reply::Fault(super::rstring_to_vec(body)),
            })
        })
    }
}
