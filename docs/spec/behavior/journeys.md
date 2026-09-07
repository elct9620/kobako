# Journeys

The walks a Host App takes end to end, each one reaching what it set out for.

## Includes

- `test/e2e/test_journeys.rb`

### Why these scenarios

Every step in these walks is declared somewhere else, and that is the point: what a journey settles is that the steps compose, which no scenario about one step can say. A dispatch that works, a taxonomy that separates, a yield that unwinds and a pool that hands out warm Sandboxes are four correct pieces that still leave a Host App unable to run model-generated code, because composing them is its own thing to get wrong.

So each journey is written as one walk with one destination, and the observation is arrival rather than mechanism. Where a walk has more than one thing to reach — a failure that must both carry a class and carry a backtrace — each is its own scenario, since a walk arriving half way is what a single observation would hide.

The reuse and isolation walks are not here. They are what a Sandbox does between invocations, so they are declared with the Sandbox and witnessed by the lifecycle tests.

## `J-001` A curated capability answers a generated script

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service bound at a name |
| When | model-generated source calls that Service |
| Then | the Host App reads the answer as a value rather than as bytes |

## `J-002` A script that fails reaches the Host App as a script failure

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | generated source raises and nothing in it rescues |
| Then | `Kobako::SandboxError` is raised, attributed to the sandbox and to neither other class |

## `J-003` Source that will not compile runs nothing first

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | generated source that writes output and then fails to parse is evaluated |
| Then | `Kobako::SandboxError` is raised, attributed to the sandbox, and nothing was written |

## `J-004` A script failure says where in the script it happened

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | generated source raises inside a method it defined |
| Then | the failure carries the guest's backtrace |

## `J-005` A capability that fails reaches the Host App as a capability failure

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service that raises |
| When | generated source calls that Service and nothing rescues |
| Then | `Kobako::ServiceError` is raised, attributed to the service and not as a script failure |

## `J-006` A capability failure says where too

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service that raises |
| When | generated source calls that Service and nothing rescues |
| Then | the failure carries the guest's backtrace |

## `J-007` Each failure a Host App routes apart arrives as its own class

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service bound at a name |
| When | source raising itself, naming no Service, mismatching the arguments, and reaching a Service that raises are each evaluated |
| Then | each raises the class its own rescue names |

## `J-008` A block-yielding Service maps a collection through guest code

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service that yields each element to a block |
| When | generated source calls it with a block |
| Then | the Host App reads the collection the guest block mapped |

## `J-009` Concurrent requests each receive their own result

| Step | Statement |
| --- | --- |
| Given | a Pool whose Sandboxes each preloaded an entrypoint |
| When | more requests than slots run that entrypoint at once, each with its own argument |
| Then | each request reads the result of its own |
