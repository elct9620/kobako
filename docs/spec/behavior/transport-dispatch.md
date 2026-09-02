# Transport dispatch

How a guest call reaches a host object, what crosses in each direction, and how long a capability reference lasts.

## Includes

- `test/unit/transport/test_dispatcher.rb`
- `test/unit/transport/test_dispatcher_handles.rb`
- `test/unit/transport/test_dispatcher_invalidity.rb`
- `test/unit/transport/test_dispatcher_violations.rb`
- `test/unit/catalog/test_handles.rb`
- `test/unit/codec/test_handle_walk.rb`
- `test/unit/codec/test_deep_restore.rb`
- `test/e2e/test_handle_arguments.rb`
- `test/e2e/test_handle_restoration.rb`
- `test/e2e/test_handle_proxy.rb`
- `test/e2e/test_dispatch_args.rb`
- `test/e2e/test_dispatch_kwargs_partition.rb`
- `test/e2e/test_dispatch_gc_safety.rb`
- `test/e2e/test_dsl_composition.rb`
- `test/e2e/sandbox/test_run_auto_wrap.rb`
- `test/parity/test_dispatch.rb`
- `test/parity/test_handles.rb`

### Why these scenarios

A guest call reaching a host object is the only route outward, so what it carries is witnessed in both directions and at three depths: the walk that decides what the wire can hold, the table that hands out references for what it cannot, and the dispatch that puts them back together.

A reference lasts one invocation and belongs to one Sandbox. Both bounds are witnessed as a receiver and as an argument, because a table consulted on one path and not the other would pass either witness alone.

The two argument kinds are separated by how the guest wrote the call and not by what the value is, so a Hash appears on both sides of that line — as a positional literal, as a splatted keyword map, and as a keyword's value — and each is witnessed.

Everything that answers on the fault arm rather than raising is here, since the dispatcher never raises; what a Host App finally rescues is the error taxonomy's to state.

## `T-001` An answer the wire cannot carry becomes a reference to it

| Step | Statement |
| --- | --- |
| Given | a bound Service whose method answers a stateful object |
| When | the guest calls it |
| Then | the guest receives a capability reference rather than the object |

## `T-002` An answer the wire can carry crosses as itself

| Step | Statement |
| --- | --- |
| Given | a bound Service whose method answers a primitive |
| When | the guest calls it |
| Then | the guest receives that value, not a reference |

## `T-003` A reference passed as a positional argument arrives as its object

| Step | Statement |
| --- | --- |
| Given | a guest holding a capability reference |
| When | it passes the reference as a positional argument to a Service |
| Then | the Service receives the host object |

## `T-004` A reference passed as a keyword argument arrives as its object

| Step | Statement |
| --- | --- |
| Given | a guest holding a capability reference |
| When | it passes the reference as a keyword argument to a Service |
| Then | the Service receives the host object |

## `T-005` A reference the table never issued is refused as an argument

| Step | Statement |
| --- | --- |
| Given | an invocation whose Handle table never issued a given id |
| When | the guest passes that id as an argument |
| Then | the dispatch answers as an undefined target |

## `T-006` A reference used as the receiver dispatches to its object

| Step | Statement |
| --- | --- |
| Given | a guest holding a capability reference |
| When | it calls a method on that reference |
| Then | the host object answers |

## `T-007` An answer from a reference is referenced in turn

| Step | Statement |
| --- | --- |
| Given | a guest holding a capability reference whose method answers a stateful object |
| When | it calls that method |
| Then | the guest receives a new capability reference |

## `T-008` A reference the table never issued is refused as a receiver

| Step | Statement |
| --- | --- |
| Given | an invocation whose Handle table never issued a given id |
| When | the guest calls a method on that id |
| Then | the dispatch answers as an undefined target |

## `T-009` References are numbered from one, upward

| Step | Statement |
| --- | --- |
| Given | a fresh Handle table |
| When | several references are allocated in turn |
| Then | their ids run upward from one |

## `T-010` A reference answers with the object it was made for

| Step | Statement |
| --- | --- |
| Given | a Handle table holding one allocation |
| When | that id is fetched |
| Then | it answers the object that was bound to it |

## `T-011` Fetching an id nobody allocated raises

| Step | Statement |
| --- | --- |
| Given | a Handle table |
| When | an id it never allocated is fetched |
| Then | it raises |

## `T-012` The table has a ceiling and stops there

| Step | Statement |
| --- | --- |
| Given | a Handle table filled to its highest id |
| When | one more allocation is attempted |
| Then | it raises |

## `T-013` The ceiling is a property of the wire, not of the table

| Step | Statement |
| --- | --- |
| Given | the Handle table's ceiling constant |
| When | it is read |
| Then | it is the value the wire format fixes |

## `T-014` A reflective object is never given a reference

| Step | Statement |
| --- | --- |
| Given | a Handle table |
| When | a reflective gadget is offered for allocation |
| Then | it is refused |

## `T-015` A callable still gets one

| Step | Statement |
| --- | --- |
| Given | a Handle table |
| When | a callable is offered for allocation |
| Then | it is allocated |

## `T-016` A reference from an earlier invocation resolves to nothing

| Step | Statement |
| --- | --- |
| Given | a reference issued by an earlier invocation's table |
| When | the next invocation's table is asked for it |
| Then | it resolves to no object |

## `T-017` An id nobody was given resolves to nothing

| Step | Statement |
| --- | --- |
| Given | an invocation's Handle table |
| When | an arbitrary integer is presented as a reference |
| Then | it resolves to no object |

## `T-018` References minted during a failed wrap leave with their table

| Step | Statement |
| --- | --- |
| Given | an invocation whose wrap failed partway |
| When | the ids it had minted are presented afterward |
| Then | they resolve to no object |

## `T-019` The wire's own scalars are recognised as themselves

| Step | Statement |
| --- | --- |
| Given | a value of each scalar kind the wire carries |
| When | the walk classifies it |
| Then | it is recognised as wire-representable |

## `T-020` A reference is wire-representable

| Step | Statement |
| --- | --- |
| Given | an existing capability reference |
| When | the walk classifies it |
| Then | it is recognised as wire-representable |

## `T-021` An integer past the wire's width is not

| Step | Statement |
| --- | --- |
| Given | an Integer outside the range the wire carries |
| When | the walk classifies it |
| Then | it is refused |

## `T-022` Nor is a scalar the wire has no shape for

| Step | Statement |
| --- | --- |
| Given | a scalar of a kind the wire does not carry |
| When | the walk classifies it |
| Then | it is refused |

## `T-023` An Array is representable exactly when its elements are

| Step | Statement |
| --- | --- |
| Given | an Array whose elements are all representable, and one where they are not |
| When | the walk classifies each |
| Then | only the first is representable |

## `T-024` A Hash is representable exactly when its keys and values are

| Step | Statement |
| --- | --- |
| Given | a Hash whose keys and values are all representable, and one where they are not |
| When | the walk classifies each |
| Then | only the first is representable |

## `T-025` A representable value is left alone

| Step | Statement |
| --- | --- |
| Given | a wire-representable value |
| When | the walk runs over it |
| Then | it passes through unchanged |

## `T-026` A leaf the wire cannot carry is handed to the wrapper

| Step | Statement |
| --- | --- |
| Given | a value the wire cannot carry |
| When | the walk runs over it |
| Then | the wrapper is asked to reference it |

## `T-027` A mixed Array only wraps what needs wrapping

| Step | Statement |
| --- | --- |
| Given | an Array holding both representable and non-representable elements |
| When | the walk runs over it |
| Then | only the non-representable elements are referenced |

## `T-028` A Hash's values are walked and its keys are not

| Step | Statement |
| --- | --- |
| Given | a Hash whose values need wrapping |
| When | the walk runs over it |
| Then | the values are referenced and the keys pass through |

## `T-029` A key the wire cannot carry fails the invocation

| Step | Statement |
| --- | --- |
| Given | a Hash keyed by a value the wire cannot carry |
| When | the walk runs over it |
| Then | `Kobako::SandboxError` is raised |

## `T-030` A reference is not referenced again

| Step | Statement |
| --- | --- |
| Given | a value already carrying a capability reference |
| When | the walk runs over it |
| Then | the reference passes through unchanged |

## `T-031` Nesting is walked a level at a time

| Step | Statement |
| --- | --- |
| Given | a container nested several levels deep |
| When | the walk runs over it |
| Then | each level is walked in turn |

## `T-032` A value carrying no reference is restored as itself

| Step | Statement |
| --- | --- |
| Given | a value holding no capability reference |
| When | the restore runs over it |
| Then | it passes through unchanged |

## `T-033` A bare reference restores to its object

| Step | Statement |
| --- | --- |
| Given | a capability reference with a live binding |
| When | the restore runs over it |
| Then | it answers the bound object |

## `T-034` An Array restores only its references

| Step | Statement |
| --- | --- |
| Given | an Array holding both references and ordinary values |
| When | the restore runs over it |
| Then | only the references become objects |

## `T-035` A Hash restores on both sides

| Step | Statement |
| --- | --- |
| Given | a Hash holding references as keys and as values |
| When | the restore runs over it |
| Then | both sides become objects |

## `T-036` Nesting is restored a level at a time

| Step | Statement |
| --- | --- |
| Given | a container nested several levels deep holding references |
| When | the restore runs over it |
| Then | each level is restored in turn |

## `T-037` A reference with no live binding fails the invocation

| Step | Statement |
| --- | --- |
| Given | a capability reference whose binding is gone |
| When | the restore runs over it |
| Then | `Kobako::SandboxError` is raised |

## `T-038` A reference does not survive its invocation

| Step | Statement |
| --- | --- |
| Given | a reference issued during an earlier invocation |
| When | the next invocation presents it as a receiver |
| Then | the dispatch answers as an undefined target |

## `T-039` A reference does not travel between Sandboxes as a receiver

| Step | Statement |
| --- | --- |
| Given | a reference issued by one Sandbox |
| When | another Sandbox presents it as a receiver |
| Then | the dispatch answers as an undefined target |

## `T-040` Nor as an argument

| Step | Statement |
| --- | --- |
| Given | a reference issued by one Sandbox |
| When | another Sandbox passes it as an argument |
| Then | the dispatch answers as an undefined target |

## `T-041` A keyword name that is not a name is a wire violation

| Step | Statement |
| --- | --- |
| Given | a dispatch whose keyword map is keyed by something other than a name |
| When | the host reads it |
| Then | it answers as a wire violation |

## `T-042` An id the table never issued is refused at the dispatch

| Step | Statement |
| --- | --- |
| Given | a dispatch naming a reference id the table never issued |
| When | the host reads it |
| Then | it answers as an undefined target |

## `T-043` A call nested past the depth bound is contained

| Step | Statement |
| --- | --- |
| Given | a dispatch nested past the depth the host accepts |
| When | the host reads it |
| Then | it answers on the fault arm as an internal failure |

## `T-044` An answer the host cannot write is the Service's failure, not the wire's

| Step | Statement |
| --- | --- |
| Given | a bound Service answering a value the host cannot encode |
| When | the guest calls it |
| Then | the failure is attributed to the Service |

## `T-045` That failure is described in the host's own words

| Step | Statement |
| --- | --- |
| Given | a bound Service answering a value the host cannot encode |
| When | the guest reads the failure's message |
| Then | it is worded by the Host Gem rather than by the codec |

## `T-046` Running out of references while wrapping takes the fault arm

| Step | Statement |
| --- | --- |
| Given | an invocation whose Handle table is exhausted |
| When | a Service answers a value needing a new reference |
| Then | the dispatch answers on the fault arm |

## `T-047` That exhaustion surfaces as a Sandbox failure

| Step | Statement |
| --- | --- |
| Given | an invocation whose Handle table is exhausted |
| When | the guest leaves the failure unrescued |
| Then | `Kobako::SandboxError` is raised |

## `T-048` A failure the host was not meant to catch is not caught

| Step | Statement |
| --- | --- |
| Given | a bound Service raising outside the ordinary error hierarchy |
| When | the guest calls it |
| Then | that failure escapes the dispatcher's rescue |

## `T-049` A reference reaches the host as its object through a real invocation

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered a stateful object |
| When | guest code passes the reference back as a positional argument |
| Then | the Service acts on the original host object |

## `T-050` And as a keyword argument

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered a stateful object |
| When | guest code passes the reference back as a keyword argument |
| Then | the Service acts on the original host object |

## `T-051` And in both positions at once

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered stateful objects |
| When | guest code passes references back positionally and by keyword together |
| Then | the Service acts on the original host objects |

## `T-052` And nested inside an Array argument

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered a stateful object |
| When | guest code passes the reference back inside an Array |
| Then | the Service acts on the original host object |

## `T-053` And nested inside a keyword's Hash value

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered a stateful object |
| When | guest code passes the reference back inside a keyword's Hash value |
| Then | the Service acts on the original host object |

## `T-054` A reference returned from the invocation becomes its object again

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered a stateful object |
| When | guest code returns the reference as the invocation's value |
| Then | the host receives the original object |

## `T-055` Even nested in a container

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered a stateful object |
| When | guest code returns the reference inside nested containers |
| Then | the host receives the original object in place |

## `T-056` Even standing as a Hash key

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered a stateful object |
| When | guest code returns the reference as a Hash key |
| Then | the host receives the original object in that position |

## `T-057` And when it comes back through a yield block

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service yields to a guest block |
| When | the block answers with a reference |
| Then | the Service receives the original object |

## `T-058` A reference the guest damaged still routes to its object

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose guest code altered what it holds around a reference |
| When | it calls a method through that reference |
| Then | the original host object answers |

## `T-059` A brace-less keyword arrives as a keyword

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service reading its keyword arguments |
| When | guest code calls it with a brace-less keyword |
| Then | the Service receives it among its keywords |

## `T-060` An explicit Hash stays positional

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service reading its positional arguments |
| When | guest code calls it with an explicit Hash literal |
| Then | the Service receives it among its positional arguments |

## `T-061` The two never mix

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service reading both argument kinds |
| When | guest code calls it with an explicit Hash and a brace-less keyword together |
| Then | each arrives in its own bucket |

## `T-062` A splatted Hash arrives as keywords

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service reading its keyword arguments |
| When | guest code calls it splatting a Hash as keywords |
| Then | the Service receives them among its keywords |

## `T-063` Splatting nothing produces no keywords

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service reading its keyword arguments |
| When | guest code calls it splatting an empty Hash |
| Then | the Service receives no keywords |

## `T-064` A keyword whose value is a Hash is still a keyword

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service reading its keyword arguments |
| When | guest code calls it with a keyword whose value is a Hash |
| Then | the Service receives the keyword carrying that Hash |

## `T-065` An empty explicit Hash is still positional

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service reading its positional arguments |
| When | guest code calls it with an empty explicit Hash literal |
| Then | the Service receives it among its positional arguments |

## `T-066` An entrypoint argument the wire cannot carry becomes a reference

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded entrypoint that calls a method on its argument |
| When | it runs with a stateful host object as a positional argument |
| Then | the guest reaches that object and the call answers |

## `T-067` And so does one passed by keyword

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded entrypoint that calls a method on a keyword's value |
| When | it runs with a stateful host object as a keyword value |
| Then | the guest reaches that object and the call answers |

## `T-068` An entrypoint argument keyed by an unwrappable value is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded entrypoint |
| When | it runs with a Hash argument keyed by a value that cannot be referenced |
| Then | `Kobako::SandboxError` is raised |

## `T-069` An argument that refers to itself is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded entrypoint |
| When | it runs with an argument containing itself |
| Then | `Kobako::SandboxError` is raised |

## `T-070` Dispatching survives collection pressure

| Step | Statement |
| --- | --- |
| Given | a Sandbox under garbage-collection stress |
| When | guest code dispatches repeatedly through references |
| Then | every dispatch answers |

## `T-071` So does unwinding out of a block under compaction

| Step | Statement |
| --- | --- |
| Given | a Sandbox under garbage-collection compaction |
| When | guest code breaks out of a yielded block |
| Then | the invocation answers the break's value |

## `T-072` A receiver-less guest idiom composes over references

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answers child objects |
| When | guest code builds a nested structure through a receiver-less idiom |
| Then | the host receives the structure the idiom described |

## `T-073` So does the block-parameter form

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answers child objects |
| When | guest code builds the same structure through a block parameter |
| Then | the host receives the structure the idiom described |

## `T-074` An idiom's vocabulary is the host's method set and no wider

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose guest idiom forwards any name to the host |
| When | guest code calls a name the host object does not define |
| Then | `Kobako::ServiceError` is raised |

## `T-075` Both frontends route a dispatch the same way

| Step | Statement |
| --- | --- |
| Given | a scenario whose guest calls a bound Service |
| When | both frontends run it |
| Then | they observe the same answer |

## `T-076` Both make a Service's own failure rescuable the same way

| Step | Statement |
| --- | --- |
| Given | a scenario whose bound Service raises and whose guest rescues it |
| When | both frontends run it |
| Then | they observe the same rescued value |

## `T-077` Both refuse an unknown method the same way

| Step | Statement |
| --- | --- |
| Given | a scenario calling a method the bound object does not define |
| When | both frontends run it |
| Then | they refuse it the same way |

## `T-078` Both attribute an argument fault the same way

| Step | Statement |
| --- | --- |
| Given | a scenario calling a bound Service with arguments it cannot accept |
| When | both frontends run it |
| Then | they attribute the failure the same way |

## `T-079` Both narrow a host object the same way

| Step | Statement |
| --- | --- |
| Given | a scenario whose bound object narrows its guest-reachable surface |
| When | both frontends run it |
| Then | they observe the same reachable surface |

## `T-080` Both carry a reference through its life the same way

| Step | Statement |
| --- | --- |
| Given | a scenario minting a reference, using it, and returning it |
| When | both frontends run it |
| Then | they observe the same values |

## `T-081` Both refuse a reference the guest tried to mint

| Step | Statement |
| --- | --- |
| Given | a scenario whose guest tries to construct a reference from an integer |
| When | both frontends run it |
| Then | they refuse it the same way |

## `T-082` Both wrap an entrypoint's unwrappable argument the same way

| Step | Statement |
| --- | --- |
| Given | a scenario running an entrypoint with a stateful host argument |
| When | both frontends run it |
| Then | they observe the same answer |

## `T-137` A dispatch to a bound path answers on the ok arm

| Step | Statement |
| --- | --- |
| Given | a registry with a Service bound at a path |
| When | a dispatch names that path and a method it answers |
| Then | the answer carries the Service's value on the ok arm |

## `T-138` Keyword names reach the Service as Symbols

| Step | Statement |
| --- | --- |
| Given | a registry with a Service recording the keywords it is called with |
| When | a dispatch carries keyword arguments |
| Then | the Service received them under Symbol keys |

## `T-139` A path nothing is bound at is an undefined target

| Step | Statement |
| --- | --- |
| Given | a registry with nothing bound at a path |
| When | a dispatch names that path |
| Then | it answers on the fault arm as an undefined target |

## `T-140` A Service's own exception is the Service's failure

| Step | Statement |
| --- | --- |
| Given | a registry with a Service whose method raises |
| When | the guest calls it |
| Then | it answers on the fault arm as a runtime failure carrying the Service's message |

## `T-141` Arguments that will not bind are an argument failure

| Step | Statement |
| --- | --- |
| Given | a registry with a Service whose method takes two positional arguments |
| When | a dispatch supplies none |
| Then | it answers on the fault arm as an argument failure |

## `T-142` No keywords is a keyword map with nothing in it

| Step | Statement |
| --- | --- |
| Given | a registry with a Service whose method takes no keywords |
| When | a dispatch carries an empty keyword map |
| Then | the call answers on the ok arm |

## `T-143` Keywords a method cannot take are an argument failure, not a runtime one

| Step | Statement |
| --- | --- |
| Given | a registry with a Service whose method takes no keywords |
| When | a dispatch carries a keyword anyway |
| Then | it answers on the fault arm as an argument failure |

## `T-144` So is a keyword the method does not name

| Step | Statement |
| --- | --- |
| Given | a registry with a Service whose method names one keyword |
| When | a dispatch carries that keyword and one more |
| Then | it answers on the fault arm as an argument failure |

## `T-145` Positional and keyword arguments arrive in their own positions

| Step | Statement |
| --- | --- |
| Given | a registry with a Service whose method takes one of each |
| When | a dispatch carries both |
| Then | the Service received each in its own position |

## `T-146` A method collecting keywords collects whichever arrive

| Step | Statement |
| --- | --- |
| Given | a registry with a Service whose method collects arbitrary keywords |
| When | a dispatch carries several |
| Then | the Service received them all unchanged |

## `T-147` A short method name survives a short keyword name

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service whose method name and keyword name are both short enough to be inline symbols |
| When | guest code calls it with that keyword |
| Then | the Service received the call under its own method name |

## `T-148` An argument the wire cannot carry is refused at the call site

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service |
| When | guest code passes a value having no wire representation, positionally or by keyword |
| Then | the guest sees a `TypeError` rather than the value's string form |

## `T-149` A Symbol argument arrives as a Symbol

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service reporting whether its argument is a Symbol |
| When | guest code passes one |
| Then | the Service reports that it is |

## `T-150` An Array answer arrives as a guest Array

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service answering an Array |
| When | guest code calls it and reads the answer's length |
| Then | it reads the Array's own length |

## `T-151` A Hash answer arrives as a guest Hash under its own keys

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service answering a Hash with Symbol keys |
| When | guest code calls it and subscripts the answer by one of those keys |
| Then | it reads the value bound under that key |

## `T-152` Nesting crosses in both directions unchanged

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service echoing what it receives |
| When | guest code passes an Array of Hashes |
| Then | the Service received that structure and the guest receives it back |

## `T-153` An argument's size is read from the value, not asked of the guest

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service echoing what it receives |
| When | guest code passes an Array whose own length method reports a larger count |
| Then | the Service received the Array's real elements |
