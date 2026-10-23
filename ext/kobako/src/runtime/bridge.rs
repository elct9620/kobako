//! The magnus bridge for the guest→host dispatch seam.
//!
//! The Ruby-Proc `DispatchHandler` and the frame-scoped `GuestYielder`
//! handle the Proc re-enters the guest through — the one place the
//! dispatch seam touches `magnus`. The wasm-side dispatch path
//! (`kobako_wasmtime`'s dispatch module) sees only the contract traits.

use core::cell::{Cell, RefCell};
use core::ptr::NonNull;
use std::sync::Arc;

use magnus::rb_sys::AsRawValue;
use magnus::value::{Opaque, ReprValue};
use magnus::{method, prelude::*, Error as MagnusError, RClass, RString, Ruby, Symbol, Value};

use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::yielder::Yielder;
use kobako_transport::envelope::{Call, Fault, FaultKind, Reply, Target, YieldReply};

/// The names the host `Transport::Yielder` branches on — one per Yield
/// Reply arm. Neutral Symbols rather than the envelope's tag bytes, the
/// same way `Snapshot#outcome` names its arms: the core envelope is this
/// side's to read, so no byte value of it reaches Ruby.
const ARM_OK: &str = "ok";
const ARM_BREAK: &str = "break";
const ARM_ERROR: &str = "error";
const ARM_UNSUPPORTED: &str = "unsupported";

/// Runs after `Kobako::Runtime` is defined, so the `#[magnus::wrap]` class
/// name resolves before any object is wrapped.
pub(super) fn register(runtime_class: RClass) -> Result<(), MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    let class = runtime_class.define_class("GuestYielder", ruby.class_object())?;
    class.define_method("call", method!(GuestYielder::call, 1))?;
    Ok(())
}

/// The Yielders one invocation's open Calls lent, innermost last. A yield
/// re-enters the guest only through the innermost, whatever Call's block
/// it runs, so no Yielder is reached while a later Call holds the guest.
///
/// This is the one place the host↔guest re-entry still costs raw
/// pointers: `magnus`' `funcall` sits between two Rust frames, so the
/// typed `&mut dyn Yielder` cannot cross it and is erased here. Each
/// pointer is pushed before its frame's `funcall` and popped after it, so
/// every pointer on the stack belongs to a frame still on the Rust stack.
#[derive(Default)]
struct OpenFrames(RefCell<Vec<NonNull<dyn Yielder>>>);

// SAFETY: the stack is pushed and popped by the dispatch frames on the
// fiber driving the invocation, and read only by a `GuestYielder` that
// refuses every other fiber, so it is never reached from another thread.
unsafe impl Send for OpenFrames {}
unsafe impl Sync for OpenFrames {}

impl OpenFrames {
    /// Lend `yielder` for the frame `run` answers, popping it however
    /// `run` returns.
    fn lend<T>(&self, yielder: &mut dyn Yielder, run: impl FnOnce() -> T) -> T {
        struct Pop<'a>(&'a OpenFrames);
        impl Drop for Pop<'_> {
            fn drop(&mut self) {
                self.0 .0.borrow_mut().pop();
            }
        }
        // SAFETY: the lifetime is erased only for as long as `Pop` keeps
        // the pointer on the stack, which ends before `yielder` does.
        let ptr: NonNull<dyn Yielder> = unsafe {
            std::mem::transmute::<NonNull<dyn Yielder + '_>, NonNull<dyn Yielder + 'static>>(
                NonNull::from(yielder),
            )
        };
        self.0.borrow_mut().push(ptr);
        let _pop = Pop(self);
        run()
    }

    fn innermost(&self) -> Option<NonNull<dyn Yielder>> {
        self.0.borrow().last().copied()
    }
}

/// Frame-scoped Ruby handle that lets the dispatch `Proc` run the block
/// one Call carried. The bridge builds one per `__kobako_upcall` frame,
/// hands it to the `Proc` as the last argument, and `invalidate`s it the
/// instant the `Proc` returns. A guest block stashed and called after that
/// frame normally raises `LocalJumpError` at the Ruby
/// `Transport::Yielder` net — invalidated in the dispatcher's `ensure`,
/// which fires before this handle is reached. This inner invalidation is
/// the backstop behind that outer net, so neither net is redundant.
///
/// The guest's Store is entered from the fiber that drives the
/// invocation, and only from it: another fiber runs on its own native
/// stack, which wasmtime's stack limit and the value budget never saw. A
/// block run from any other fiber, including one on another thread, is
/// refused before it reaches the guest.
///
/// It holds **no Ruby `Value`**, so GC has nothing to trace through it —
/// it needs no `mark`. The driving fiber is kept as its raw identity,
/// compared only while the frame is live and so while that fiber is.
#[magnus::wrap(class = "Kobako::Runtime::GuestYielder", free_immediately, size)]
struct GuestYielder {
    frame: u32,
    open: Arc<OpenFrames>,
    live: Cell<bool>,
    fiber: rb_sys::VALUE,
}

// SAFETY: magnus requires `Send + Sync` on wrapped types. `call` refuses
// every fiber but the one driving the invocation, so the frames it reaches
// are only ever read on that fiber's thread.
unsafe impl Send for GuestYielder {}
unsafe impl Sync for GuestYielder {}

impl GuestYielder {
    fn new(ruby: &Ruby, frame: u32, open: Arc<OpenFrames>) -> Self {
        Self {
            frame,
            open,
            live: Cell::new(true),
            fiber: ruby.fiber_current().as_raw(),
        }
    }

    /// Runs the instant the dispatch frame's `funcall` returns, so a guest
    /// block stashed beyond its frame raises instead of re-entering.
    fn invalidate(&self) {
        self.live.set(false);
    }

    /// The arm is named here off the decoded Yield Reply, so the host
    /// `Transport::Yielder` decodes only a payload.
    fn call(&self, args: RString) -> Result<(Symbol, RString, Option<String>), MagnusError> {
        let ruby = Ruby::get().expect("Ruby handle unavailable in the block bridge");
        let innermost = self.open.innermost().filter(|_| self.live.get());
        let Some(mut ptr) = innermost else {
            return Err(super::errors::trap_err(
                &ruby,
                "guest block invoked after the host dispatch frame returned",
            ));
        };
        if ruby.fiber_current().as_raw() != self.fiber {
            return Err(MagnusError::new(
                ruby.exception_thread_error(),
                "guest block called from a thread or fiber other than its invocation's",
            ));
        }
        let bytes = super::rstring_to_vec(args);
        // SAFETY: this frame is live, so the innermost frame — this one or
        // a later Call — is still on the Rust stack with its Yielder lent.
        // Only the innermost frame's Ruby code runs, so no other borrow of
        // that Yielder is live; the borrow ends with this method.
        let yielder: &mut dyn Yielder = unsafe { ptr.as_mut() };
        let reply = yielder
            .yield_to_block(self.frame, &bytes)
            .map_err(|t| super::errors::trap_to_magnus(&ruby, t))?;
        Ok(match reply {
            YieldReply::Ok(body) => (ruby.to_symbol(ARM_OK), ruby.str_from_slice(&body), None),
            YieldReply::Break => (ruby.to_symbol(ARM_BREAK), ruby.str_from_slice(&[]), None),
            YieldReply::Error(record) => (
                ruby.to_symbol(ARM_ERROR),
                ruby.str_new(&record.message),
                Some(record.name),
            ),
            // The guest's own message stays behind: the host words this
            // refusal itself.
            YieldReply::Unsupported(_) => (
                ruby.to_symbol(ARM_UNSUPPORTED),
                ruby.str_from_slice(&[]),
                None,
            ),
        })
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
    open: Arc<OpenFrames>,
}

impl RubyDispatchHandler {
    pub(super) fn new(on_dispatch: Opaque<Value>) -> Self {
        Self {
            on_dispatch,
            open: Arc::default(),
        }
    }
}

impl DispatchHandler for RubyDispatchHandler {
    /// Ruby decodes only the payload, through the codec it already owns,
    /// which keeps a large payload's strings shared with the buffer rather
    /// than copied. The Proc folds every `StandardError` into the Reply's
    /// fault arm, so one escaping is a contract violation surfaced as
    /// `None`.
    fn dispatch(&self, call: Call<'_>, yielder: &mut dyn Yielder) -> Option<Reply> {
        // The guest may be running GVL-free (`gvl: :release`); re-acquire the
        // GVL for the whole callback before touching any Ruby VALUE. In hold
        // mode `reenter` runs the body inline — the GVL is already held.
        super::gvl::reenter(|| {
            // The wasmtime callback runs on the same Ruby thread that called
            // the active Sandbox invocation (#eval or #run), so `Ruby::get()`
            // is available here once the GVL is held.
            // Panicking with `expect` localises the violation rather than
            // letting a nonsense error propagate.
            let ruby = Ruby::get().expect("Ruby handle unavailable in __kobako_upcall");
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
            // escapes the dispatch frame never re-enters the guest.
            let guest_yielder =
                ruby.obj_wrap(GuestYielder::new(&ruby, yielder.frame(), self.open.clone()));
            let answer: Result<(bool, RString, Option<RString>), magnus::Error> =
                self.open.lend(yielder, || {
                    proc_value.funcall(
                        "call",
                        (
                            target,
                            ruby.str_new(call.method),
                            call.block_given,
                            ruby.str_from_slice(call.payload),
                            guest_yielder,
                        ),
                    )
                });
            guest_yielder.invalidate();
            // The Proc rescues only StandardError, so what else escapes it
            // is the host's own and is held for the host; a StandardError
            // escaping breaks the Proc's contract.
            let (ok, body, kind) = match answer {
                Ok(answer) => answer,
                Err(error) => {
                    let _ = super::escape::hold(&ruby, error);
                    return None;
                }
            };
            // The ok arm carries payload bytes the codec owns; the fault
            // arm carries the Fault's own two fields, which the envelope
            // types — so Ruby names a category rather than encoding one.
            // A block exit names the break arm in that slot, since nothing
            // of it crosses. A category outside the closed set cannot come
            // from the public API, so it fails the dispatch rather than
            // degrading.
            if ok {
                return Some(Reply::Ok(super::rstring_to_vec(body)));
            }
            let kind = kind?.to_string().ok()?;
            if kind == ARM_BREAK {
                return Some(Reply::Break);
            }
            Some(Reply::Fault(Fault::new(
                FaultKind::from_name(&kind)?,
                body.to_string().ok()?,
            )))
        })
    }

    fn abandoned(&self) -> bool {
        super::escape::pending()
    }
}
