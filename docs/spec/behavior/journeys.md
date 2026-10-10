# Journeys

The walks a Host App takes end to end, each one reaching what it set out for.

### Why these scenarios

Every step in these walks is declared somewhere else, and that is the point. A journey settles that the steps compose. No scenario about one step can say that. Four correct pieces can still leave a Host App unable to run model-generated code:

- A dispatch that works.
- A taxonomy that separates.
- A yield that unwinds.
- A pool that hands out warm Sandboxes.

Composing them is its own thing to get wrong.

So each journey is written as one walk with one destination, and the observation is arrival rather than mechanism.

A walk may have more than one thing to reach. A failure, for example, must carry both a class and a backtrace. Each such thing is its own scenario. A single observation would hide a walk arriving half way.

A walk that reuses one Sandbox across requests arrives somewhere a Sandbox scenario also states. So the lifecycle test that walks it claims both: the Sandbox scenario for the step, the journey for the arrival.

## Includes

- `test/e2e/test_journeys*.rb`
- `test/e2e/test_lifecycle.rb`

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
| Then | it fails as a Sandbox failure, attributed to the sandbox and to neither other side |

## `J-003` Source that will not compile runs nothing first

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | generated source that writes output and then fails to parse is evaluated |
| Then | it fails as a Sandbox failure, attributed to the sandbox |

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
| Then | it fails as a Service failure, attributed to the service and not as a script failure |

## `J-006` A capability failure carries the guest's backtrace

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

## `J-010` Source that will not compile says where the parse stopped

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | generated source that fails to parse is evaluated |
| Then | the failure's message names the line and column the parse stopped at |

## `J-011` An installed idiom answers a script that needs both sides

| Step | Statement |
| --- | --- |
| Given | a Sandbox with an Extension whose idiom builds paths in the guest and whose backend reads a host store |
| When | a script builds a path and reads what the store holds there |
| Then | the Host App reads what the store held at that path |

## `J-012` A runaway submission costs the others nothing

| Step | Statement |
| --- | --- |
| Given | submissions evaluated one after another under a deadline, one of which never ends |
| When | every submission has been evaluated |
| Then | each submission that ends reaches the operator with its own result |

## `J-013` A Sandbox set up once serves request after request

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service bound once at setup |
| When | each request evaluates source calling that Service on the same Sandbox |
| Then | each request reads its own answer, with nothing bound again |

## `J-014` A request's log reaches the Host App beside its result

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | a request evaluates source that writes a log line and answers a value |
| Then | the Host App reads the value and the log line apart |

## `J-015` A user expression decides each event

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service exposing the event at hand |
| When | a user expression over that event is evaluated per event |
| Then | each evaluation answers the decision the platform branches on |

## `J-016` A preloaded worker serves many requests

| Step | Statement |
| --- | --- |
| Given | a Sandbox that preloaded a worker entrypoint once |
| When | the worker runs per request, each with its own arguments and keywords |
| Then | each request reads the worker's answer for its own arguments |

## `J-017` Source that will not compile writes nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | generated source that writes output and then fails to parse is evaluated |
| Then | the failure's Execution captured no output |
