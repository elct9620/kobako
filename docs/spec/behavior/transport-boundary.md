# Dispatch boundary

What the host refuses to dispatch, and how narrow a bound object can make its own reachable surface.

## Includes

- `test/e2e/test_handle_proxy.rb`
- `test/e2e/test_handle_immutable.rb`
- `test/e2e/test_proxy_target.rb`
- `test/e2e/test_reflection_block.rb`
- `test/e2e/test_class_escape.rb`
- `test/unit/transport/test_dispatcher_allowlist.rb`
- `test/unit/transport/test_dispatcher_gadget_return.rb`
- `test/unit/transport/test_dispatcher_permissive_return.rb`
- `test/unit/transport/test_dispatcher_narrowing.rb`
- `test/parity/test_reflection.rb`
- `test/unit/values/test_handle.rb`
- `crates/kobako/src/dispatch.rs`
- `crates/kobako/src/msgpack/receiver.rs`
- `wasm/kobako-mruby/src/runtime/bridges.rs`

### Why these scenarios

The host is the boundary. Every refusal here is witnessed where the host decides it, and the guest-side mirror is witnessed separately as a convenience rather than as the thing that holds — a guest that skipped its own check would still be refused. A reflective object returned from a host method has no parity scenario: only one frontend has such objects to return, so that refusal is witnessed on that frontend alone.

Refusal turns on who owns the method rather than on how it is spelled, so a bound object defining a method whose name matches a refused one is answered by its own. Without that scenario the rule would read as a list of forbidden words.

Narrowing sits beneath the boundary, never above it: an object may close its surface as far as it likes and may not open what the boundary closed. Both directions are witnessed, along with the predicate staying unreachable — a narrowing an object could be asked to describe would be a surface of its own.

### Behaviors without a witness

Constructing an instance of a bound proxy, by either construction entry, succeeds in the guest without reaching the host.

A method called on such an instance raises `NoMethodError` in the guest.

A reflective gadget bound as a Service answers none of its own reflective methods, evaluation included.

Every name on the callable allowlist reaches a bound callable, not only the name that calls it.

An ordinary object bound as a Service keeps the singleton methods defined on it reachable; only a bound class or module loses its class-level surface.

A reflective gadget nested inside a container that crossed as a capability reference is refused when the guest extracts it.

A reflective name on a capability reference to a callable is refused like one on a bound callable.

The guest's proxy refuses a reflective name on a capability reference just as it does on a bound constant.

The guest proxy's refusal is a `NoMethodError` the guest may rescue.

A name the narrowing predicate permits but the object has no method for is still refused as an undefined target.

An object narrowed to nothing can still be held by the guest, passed as a dispatch argument, and returned across the boundary.

A narrowed name left unrescued fails as a Service failure.

A subclass of the reference type, or a guest module mixing in the forwarding seam, is refused in the guest before the host is asked.

Reassigning a held reference's identifier through instance evaluation raises `FrozenError`.

A clone of a held reference is frozen too.

A copy of a held reference keeps its identifier and dispatches to the same host object.

## `T-108` A guest may ask whether a name is reachable

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a capability reference in guest hands |
| When | guest code probes it for a method its object defines |
| Then | it reports that it responds to it |

## `T-109` Constructing a proxy is not acquiring a capability

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service |
| When | guest code constructs an instance of the bound proxy |
| Then | that instance carries no dispatch of its own |

## `T-110` A capability reference cannot be constructed

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code tries to construct a reference directly |
| Then | the construction is refused |

## `T-111` A reference the guest holds is frozen

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered a stateful object |
| When | guest code asks the reference whether it is frozen |
| Then | it is |

## `T-112` Being frozen does not stop it working

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered a stateful object |
| When | guest code dispatches through the frozen reference |
| Then | the host object answers |

## `T-113` An object that only looks like a reference is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose guest built an object carrying the reference's shape |
| When | guest code dispatches through it |
| Then | the guest refuses it before the host is asked |

## `T-114` The guest's own proxy refuses a reflective name

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service |
| When | guest code calls a reflective name on the bound proxy |
| Then | the proxy refuses it |

## `T-115` A callable the host permitted still forwards

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a callable bound as a Service |
| When | guest code calls it through the proxy |
| Then | the callable answers |

## `T-116` A meta-programming name is refused, not dispatched

| Step | Statement |
| --- | --- |
| Given | a dispatch naming a meta-programming method on a bound target |
| When | the host reads it |
| Then | it is refused rather than forwarded |

## `T-117` A reflective name is refused too

| Step | Statement |
| --- | --- |
| Given | a dispatch naming a reflective method on a bound target |
| When | the host reads it |
| Then | it is refused rather than forwarded |

## `T-118` The permitted callable names still dispatch

| Step | Statement |
| --- | --- |
| Given | a dispatch naming a permitted method on a callable target |
| When | the host reads it |
| Then | the callable answers |

## `T-119` A Service's own method still dispatches

| Step | Statement |
| --- | --- |
| Given | a dispatch naming a method the bound object defines itself |
| When | the host reads it |
| Then | the object answers |

## `T-120` Refusal is decided by who owns the method, not by its name

| Step | Statement |
| --- | --- |
| Given | a bound object defining a method whose name matches a refused one |
| When | the guest calls it |
| Then | the object's own method answers |

## `T-121` A name nobody defines is refused as an undefined target

| Step | Statement |
| --- | --- |
| Given | a dispatch naming a method the bound object does not define |
| When | the host reads it |
| Then | it answers as an undefined target |

## `T-122` A reflective object is refused as an answer, not referenced

| Step | Statement |
| --- | --- |
| Given | a bound Service whose method answers a reflective gadget |
| When | the guest calls it |
| Then | the answer is refused rather than given a reference |

## `T-123` A callable answer is still referenced

| Step | Statement |
| --- | --- |
| Given | a bound Service whose method answers a callable |
| When | the guest calls it |
| Then | the guest receives a capability reference |

## `T-124` An object may make itself unreachable by every name

| Step | Statement |
| --- | --- |
| Given | a bound object whose narrowing predicate denies every name |
| When | the guest calls any of its methods |
| Then | each is refused |

## `T-125` Narrowing follows the object through a reference

| Step | Statement |
| --- | --- |
| Given | a narrowing object reached as a capability reference |
| When | the guest calls a method it denies |
| Then | it is refused |

## `T-126` An object may permit exactly the names it chooses

| Step | Statement |
| --- | --- |
| Given | a bound object whose narrowing predicate permits a subset |
| When | the guest calls a permitted name and a denied one |
| Then | only the permitted one answers |

## `T-127` Narrowing cannot open what the boundary closed

| Step | Statement |
| --- | --- |
| Given | a bound object whose narrowing predicate permits a reflective name |
| When | the guest calls that name |
| Then | it is still refused |

## `T-128` A permitted name the object answers dynamically still runs

| Step | Statement |
| --- | --- |
| Given | a bound object permitting a name it handles dynamically |
| When | the guest calls that name |
| Then | the object answers |

## `T-129` An object that narrows nothing keeps its whole surface

| Step | Statement |
| --- | --- |
| Given | a bound object carrying no narrowing predicate |
| When | the guest calls its methods |
| Then | they answer as an ordinary Service's would |

## `T-130` The narrowing predicate is not itself reachable

| Step | Statement |
| --- | --- |
| Given | a bound object carrying a narrowing predicate |
| When | the guest calls the predicate by name |
| Then | it is refused |

## `T-131` Both frontends refuse a reflective target the same way

| Step | Statement |
| --- | --- |
| Given | a scenario calling a reflective name on a bound target |
| When | both frontends run it |
| Then | they refuse it the same way |

## `T-132` A bare class used as a type tag is refused as an answer

| Step | Statement |
| --- | --- |
| Given | a bound Service whose method answers a bare class or module |
| When | the guest calls it |
| Then | the answer is refused rather than given a reference |

## `T-133` A class bound directly cannot be reached through its class-level surface

| Step | Statement |
| --- | --- |
| Given | a class or module bound directly as a Service |
| When | guest code calls one of its class-level methods |
| Then | the call is refused rather than forwarded |

## `T-160` An object that answers everything still crosses as a reference

| Step | Statement |
| --- | --- |
| Given | a bound Service answering an object whose missing-method handler answers any call |
| When | the guest calls it |
| Then | the answer crosses as a capability reference |

## `T-161` A Host App cannot turn an integer into a capability reference

| Step | Statement |
| --- | --- |
| Given | a Host App holding an integer |
| When | it calls the reference type's constructor with that integer |
| Then | `NoMethodError` is raised |

## `T-162` Nor derive one carrying an identifier it chose

| Step | Statement |
| --- | --- |
| Given | a Host App holding a legitimate capability reference |
| When | it asks that reference for a copy carrying another identifier |
| Then | `NoMethodError` is raised |

## `T-182` The guest's denylist names every reflective escape

| Step | Statement |
| --- | --- |
| Given | the names a guest could reach host internals or run its own source through |
| When | the guest's denylist is read |
| Then | each of them is on it |

## `T-183` And leaves the callable names off

| Step | Statement |
| --- | --- |
| Given | the names by which a bound callable is invoked |
| When | the guest's denylist is read |
| Then | none of them is on it |

## `T-190` A probe answers yes even for a name the host object lacks

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service bound |
| When | guest code probes the bound proxy for a name the bound object does not define |
| Then | it reports that it responds to it |

## `T-191` Refusing to construct a reference is a `NoMethodError`

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code tries to construct a reference directly, with an identifier or by allocation |
| Then | `NoMethodError` is raised in the guest |

## `T-192` An object that only looks like a reference is refused as a `NoMethodError`

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose guest built an object carrying the reference's shape |
| When | guest code dispatches through it |
| Then | `NoMethodError` is raised in the guest |

## `T-193` A refused reflective name answers as an undefined target

| Step | Statement |
| --- | --- |
| Given | a dispatch naming a reflective method on a bound target |
| When | the host reads it |
| Then | it answers as an undefined target |

## `T-194` A refused class-level call left unrescued is the Service's failure

| Step | Statement |
| --- | --- |
| Given | a class or module bound directly as a Service |
| When | guest code calls one of its class-level methods and leaves the refusal unrescued |
| Then | it fails as a Service failure |

## `T-195` A refused answer is the Service's runtime failure

| Step | Statement |
| --- | --- |
| Given | a bound Service whose method answers a reflective gadget or a bare class |
| When | the guest calls it |
| Then | it answers on the fault arm as a runtime failure |

## `T-196` The guest proxy's refusal is the Sandbox's failure, not the Service's

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service |
| When | guest code calls a reflective name on the bound proxy and leaves the refusal unrescued |
| Then | it fails as a Sandbox failure |

## `T-197` A narrowed name answers as an undefined target

| Step | Statement |
| --- | --- |
| Given | a bound object whose narrowing predicate denies a name |
| When | the guest calls that name |
| Then | it answers as an undefined target |

## `T-198` A permitted name that fails while running is a runtime failure, not a narrowing

| Step | Statement |
| --- | --- |
| Given | a bound object permitting a name it handles dynamically but cannot satisfy |
| When | the guest calls that name |
| Then | it answers on the fault arm as a runtime failure |

## `T-199` Re-pointing a held reference is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered a stateful object |
| When | guest code reflectively reassigns the reference's identifier |
| Then | `FrozenError` is raised in the guest |

## `T-200` A copy of a held reference is frozen too

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered a stateful object |
| When | guest code asks a duplicate of the reference whether it is frozen |
| Then | it is |

## `T-201` A refused answer is worded as kobako's own

| Step | Statement |
| --- | --- |
| Given | a bound Service whose method answers a reflective gadget |
| When | the guest reads the failure's message |
| Then | it does not wear the shape a Service's own exception crosses in |
