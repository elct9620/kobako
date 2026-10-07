# Dispatch boundary

What the host refuses to dispatch, and which methods a host object's Exposure lets the guest reach.

### Why these scenarios

The host is the boundary. Every refusal here is witnessed where the host decides it, and the guest-side mirror is witnessed separately as a convenience rather than as the thing that holds — a guest that skipped its own check would still be refused. A reflective object returned from a host method has no parity scenario: only one frontend has such objects to return, so that refusal is witnessed on that frontend alone.

Refusal turns on who owns the method rather than on how it is spelled, so a bound object defining a method whose name matches a refused one is answered by its own. Without that scenario the rule would read as a list of forbidden words.

An Exposure sits beneath the boundary, never above it: an object may close its surface as far as it likes and may not open what the boundary closed. Both directions are witnessed, along with the predicate staying unreachable — a narrowing an object could be asked to describe would be a surface of its own.

An object carrying no narrowing predicate exposes what its own class and the object itself define, and nothing it acquired from elsewhere. The methods a Host App cannot foresee handing over are the ones it never wrote — inherited, mixed in, built into the platform, or forwarded — so the default is drawn around authorship rather than around a list of what is dangerous, and a new source of ambient methods needs no new refusal.

## Includes

- `test/e2e/test_handle_proxy.rb`
- `test/e2e/test_handle_immutable.rb`
- `test/e2e/test_proxy_target.rb`
- `test/e2e/test_reflection_block.rb`
- `test/e2e/test_class_escape.rb`
- `test/e2e/test_delegator_escape.rb`
- `test/e2e/test_own_surface.rb`
- `test/e2e/test_narrowed_reference.rb`
- `test/unit/transport/test_dispatcher_reflection.rb`
- `test/unit/transport/test_dispatcher_gadget_return.rb`
- `test/unit/transport/test_dispatcher_permissive_return.rb`
- `test/unit/transport/test_dispatcher_narrowing.rb`
- `test/unit/transport/test_dispatcher_callables.rb`
- `test/unit/transport/test_dispatcher_singletons.rb`
- `test/unit/transport/test_exposure.rb`
- `test/parity/test_reflection.rb`
- `test/unit/values/test_handle.rb`
- `crates/kobako/src/dispatch.rs`
- `crates/kobako/src/msgpack/receiver.rs`
- `wasm/kobako-mruby/src/runtime/bridges.rs`

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

## `T-129` An object that narrows nothing exposes the methods it defines itself

| Step | Statement |
| --- | --- |
| Given | a bound object carrying no narrowing predicate |
| When | the guest calls a method its own class defines |
| Then | it answers as an ordinary Service's would |

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

## `T-206` A transparent forwarder is refused as a reference

| Step | Statement |
| --- | --- |
| Given | a bound Service that answers, or a run that receives as an argument, an object that transparently forwards unknown calls to a wrapped object |
| When | it would cross as a capability reference |
| Then | it is refused rather than given a reference |

## `T-207` A forwarder's dynamic-dispatch hook is not Service behaviour

| Step | Statement |
| --- | --- |
| Given | a transparent forwarder bound directly as a Service |
| When | guest code calls its dynamic-dispatch hook explicitly |
| Then | the call is refused rather than forwarded |

## `T-208` A method inherited from a superclass is not exposed

| Step | Statement |
| --- | --- |
| Given | a bound object carrying no narrowing predicate |
| When | the guest calls a method its class inherits from a superclass |
| Then | it answers as an undefined target |

## `T-209` A method mixed in from a module is not exposed

| Step | Statement |
| --- | --- |
| Given | a bound object carrying no narrowing predicate |
| When | the guest calls a method its class gains by mixing in a module |
| Then | it answers as an undefined target |

## `T-210` A core object reached through a reference exposes nothing

| Step | Statement |
| --- | --- |
| Given | a run receiving a core object as an argument |
| When | the guest calls a method the platform builds into it |
| Then | it answers as an undefined target |

## `T-211` A record exposes the reader of each member

| Step | Statement |
| --- | --- |
| Given | a bound object of a record class declaring named members |
| When | the guest calls a member's reader |
| Then | the member's value answers |

## `T-212` A record exposes no built-in writer

| Step | Statement |
| --- | --- |
| Given | a bound object of a record class whose platform builds a writer for each member |
| When | the guest calls a member's writer |
| Then | it answers as an undefined target |

## `T-213` A method defined after the binding is not reachable through it

| Step | Statement |
| --- | --- |
| Given | a bound object carrying no narrowing predicate |
| Given | a method defined on it after the binding was made |
| When | the guest calls that method |
| Then | it answers as an undefined target |

## `T-214` A forwarder bound directly answers none of its forwarded names

| Step | Statement |
| --- | --- |
| Given | a transparent forwarder bound directly, wrapping an object that permits every name |
| When | the guest calls a name the forwarder would forward |
| Then | it answers as an undefined target |

## `T-215` A dynamically answered name needs the object's own predicate

| Step | Statement |
| --- | --- |
| Given | a bound object answering every name dynamically and carrying no narrowing predicate |
| When | the guest calls a name it would answer |
| Then | it answers as an undefined target |

## `T-220` A look-alike reference carries none across as a value

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose guest built an object carrying the reference's shape and naming an identifier the host issued |
| When | guest code hands it across as a dispatch argument |
| Then | the guest refuses it and the object that identifier names is never reached |

## `T-221` The copy hook takes the one reference it copies from

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a capability reference in guest hands |
| When | guest code reaches its copy hook with two arguments |
| Then | it is refused for its argument count |

## `T-223` A reflective gadget bound as a Service answers none of its own methods

| Step | Statement |
| --- | --- |
| Given | a reflective gadget bound as a Service |
| When | the guest calls any of its reflective methods, evaluation included |
| Then | each is refused as an undefined target |

## `T-224` Every name a bound callable keeps reaches it

| Step | Statement |
| --- | --- |
| Given | a callable bound as a Service |
| When | the guest calls each name the callable keeps |
| Then | each reaches the callable and answers |

## `T-225` A reference to a callable refuses reflection as a bound one does

| Step | Statement |
| --- | --- |
| Given | the guest holding a capability reference to a callable |
| When | it calls a reflective name on that reference |
| Then | it is refused as an undefined target |

## `T-226` A gadget pulled out of a container reference is refused

| Step | Statement |
| --- | --- |
| Given | a container that crossed as a capability reference and holds a reflective gadget |
| When | the guest extracts the gadget from it |
| Then | the answer is refused as a runtime failure and no reference is made for it |

## `T-227` An ordinary object keeps its singleton methods

| Step | Statement |
| --- | --- |
| Given | an ordinary object bound as a Service with a method defined on the object itself |
| When | the guest calls that method |
| Then | it answers |

## `T-228` Permitting a name does not conjure a method for it

| Step | Statement |
| --- | --- |
| Given | a bound object whose narrowing predicate permits every name, and which has no fallback for missing methods |
| When | the guest calls a name the object has no method for |
| Then | it is refused as an undefined target |

## `T-234` Building an instance of a bound constant stays in the guest

| Step | Statement |
| --- | --- |
| Given | a Sandbox with an object bound at a path |
| When | guest code builds an instance of the bound constant by either construction entry |
| Then | it succeeds and the object is never called |

## `T-235` Such an instance has no methods

| Step | Statement |
| --- | --- |
| Given | an instance guest code built from a bound constant |
| When | guest code calls a method on it |
| Then | it raises `NoMethodError` in the guest |

## `T-236` A reference refuses a reflective name as a bound constant does

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose guest holds a reference to a callable |
| When | guest code calls a reflective name on the reference |
| Then | the guest's proxy refuses it, while `call` still forwards |

## `T-237` The proxy's refusal is one the guest may rescue

| Step | Statement |
| --- | --- |
| Given | a guest proxy refusing a reflective name |
| When | guest code rescues `NoMethodError` around the call |
| Then | the rescue receives the refusal |

## `T-238` A reference narrowed to nothing still travels

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answers an object whose predicate permits no name |
| When | guest code holds it, passes it to another Service, and answers it |
| Then | the Service and the Host App each receive the original object |

## `T-239` A narrowed name left unrescued is a Service failure

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose guest holds an object whose predicate permits no name |
| When | guest code calls a name on it without rescuing |
| Then | the invocation fails as a Service failure |

## `T-240` Rewriting a held reference by instance evaluation is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose guest holds a reference |
| When | guest code reassigns its identifier through instance evaluation |
| Then | it raises `FrozenError` |

## `T-241` A clone of a held reference is frozen

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose guest holds a reference |
| When | guest code clones it |
| Then | the clone is frozen |

## `T-242` A copy of a held reference is the same reference

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose guest holds a reference |
| When | guest code copies it |
| Then | the copy keeps its identifier and dispatches to the same host object |

## `T-243` An impostor of the reference type is refused in the guest

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service |
| When | guest code reaches for it through a subclass of the reference type, or a guest module mixing in the forwarding seam |
| Then | it raises `NoMethodError` in the guest and the Service is never asked |

## `T-251` A method its own class keeps non-public is not exposed

| Step | Statement |
| --- | --- |
| Given | a bound object whose class defines a private and a protected method |
| When | the guest calls either name |
| Then | it answers as an undefined target |

## `T-252` A name the predicate permits reaches an inherited method

| Step | Statement |
| --- | --- |
| Given | a bound object whose narrowing predicate permits a name it inherits |
| When | the guest calls that name |
| Then | the inherited method answers |

## `T-256` A forwarder that narrows itself is asked rather than emptied

| Step | Statement |
| --- | --- |
| Given | a transparent forwarder defining its own narrowing predicate |
| When | the guest calls a name that predicate permits |
| Then | the name is exposed |

## `T-257` An object without reflection of its own still exposes what it defines

| Step | Statement |
| --- | --- |
| Given | a bound object whose class defines none of the reflection the language usually provides |
| When | the guest calls a method that class defines |
| Then | the name is exposed |

## `T-258` A method defined on one object stays with it

| Step | Statement |
| --- | --- |
| Given | two objects of one class, one carrying a method defined on the object itself |
| When | the guest calls that name on each |
| Then | only the object carrying it exposes the name |
