# Service registration

Where a host object becomes a name the guest can reach, and who may change what stands behind that name.

## Includes

- `test/unit/catalog/test_services.rb`
- `test/unit/context/test_lookup.rb`
- `test/e2e/test_bind_paths.rb`
- `test/e2e/test_fillable.rb`
- `test/e2e/test_ctx_bind.rb`
- `test/parity/test_fillable.rb`

### Why these scenarios

A binding is observable twice over: on the host, where a path resolves to an object, and in the guest, where the same path has become a constant to name. Both are witnessed, because a path that resolves correctly on the host and materializes under the wrong module reaches nobody.

The collision scenarios are three refusals and one survival. The survival is separate because a registry that raises and half-applies the bind would pass all three refusals.

A path declared without an object and a name never declared at all fail differently, and that difference is the point: one is a capability the host has reserved, the other is nothing. The override scenarios then cover who may change what stands behind a declared name, and for how long — never for longer than one invocation, and never for a name that was not already there.

A malformed path segment and a bind after the seal both raise, so they belong to the error taxonomy and are specified there. The declared path set that every invocation ships belongs to the invocation, not to registration.

## `SV-001` A bound path answers with what was bound to it

| Step | Statement |
| --- | --- |
| Given | a registry with nothing bound |
| When | an object is bound at a multi-segment path |
| Then | that path resolves to the object |

## `SV-002` Binding chains

| Step | Statement |
| --- | --- |
| Given | a registry with nothing bound |
| When | an object is bound at a path |
| Then | the bind answers the registry it was called on |

## `SV-003` A single segment is a whole path

| Step | Statement |
| --- | --- |
| Given | a registry with nothing bound |
| When | an object is bound at a single-segment path |
| Then | that path resolves to the object |

## `SV-004` A path is what it spells, not what it is written as

| Step | Statement |
| --- | --- |
| Given | a registry holding one path bound as a Symbol and another as a String |
| When | each is resolved by its String form |
| Then | each answers the object bound under it |

## `SV-005` A Service is whatever answers the call

| Step | Statement |
| --- | --- |
| Given | a registry with nothing bound |
| When | a class, an instance and a module are each bound at their own path |
| Then | all three paths resolve to the objects bound at them |

## `SV-006` A single-segment path reaches the guest with no namespace

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service bound at a single-segment path |
| When | guest code names that path as a top-level constant and calls it |
| Then | the Service's answer comes back |

## `SV-007` Each prefix segment becomes a guest module

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service bound at a three-segment path |
| When | guest code walks the module chain the prefix spells and calls the leaf |
| Then | the Service's answer comes back |

## `SV-008` Leaves under one namespace stand beside each other

| Step | Statement |
| --- | --- |
| Given | a Sandbox with two Services bound under the same prefix |
| When | guest code calls each leaf by its full path |
| Then | each answers from its own Service |

## `SV-009` A namespace is the whole prefix, not its first segment

| Step | Statement |
| --- | --- |
| Given | a Sandbox with Services bound so that two share a two-segment prefix and two more meet only at their root segment |
| When | guest code calls each leaf by its full path |
| Then | each answers from its own Service |

## `SV-010` Unrelated paths do not see each other

| Step | Statement |
| --- | --- |
| Given | a registry holding Services at two unrelated paths |
| When | each path is resolved |
| Then | each answers its own object |

## `SV-011` Siblings under one prefix do not collide

| Step | Statement |
| --- | --- |
| Given | a registry with a Service bound under a prefix |
| When | a second Service is bound under the same prefix at a different leaf |
| Then | both paths resolve to their own objects |

## `SV-012` A path is bound once

| Step | Statement |
| --- | --- |
| Given | a registry with a Service bound at a path |
| When | the same path is bound again |
| Then | `ArgumentError` is raised |

## `SV-013` A name is a Service or a grouping, never both

| Step | Statement |
| --- | --- |
| Given | a registry with a Service bound at a single-segment path |
| When | a path extending it is bound |
| Then | `ArgumentError` is raised |

## `SV-014` A grouping cannot become a Service either

| Step | Statement |
| --- | --- |
| Given | a registry with a Service bound at a two-segment path |
| When | that path's prefix is bound |
| Then | `ArgumentError` is raised |

## `SV-015` A refused bind changes nothing

| Step | Statement |
| --- | --- |
| Given | a registry with a Service bound at a path |
| Given | a bind refused for colliding with it |
| When | the original path is resolved |
| Then | it answers the object bound first |

## `SV-016` An unfilled path is a path that resolves to nothing

| Step | Statement |
| --- | --- |
| Given | an invocation whose registry declares a path with no object |
| When | that path is resolved |
| Then | it reports as unresolvable |

## `SV-017` A declared but unfilled path fails closed

| Step | Statement |
| --- | --- |
| Given | a Sandbox declaring a Service path with no object |
| When | guest code calls that path and leaves the failure unrescued |
| Then | `Kobako::ServiceError` is raised |

## `SV-018` The sentinel is the declaration, spelled out

| Step | Statement |
| --- | --- |
| Given | a Sandbox binding `Kobako::Unresolved` at a path explicitly |
| When | guest code calls that path and leaves the failure unrescued |
| Then | `Kobako::ServiceError` is raised |

## `SV-019` A declared name and an unknown one are told apart

| Step | Statement |
| --- | --- |
| Given | a Sandbox declaring one Service path with no object |
| When | guest code names a constant that was never declared |
| Then | `Kobako::SandboxError` is raised |

## `SV-020` The guest may carry on past an unfilled path

| Step | Statement |
| --- | --- |
| Given | a Sandbox declaring a Service path with no object |
| When | guest code calls that path inside a rescue and returns a value |
| Then | the invocation answers that value |

## `SV-021` An override outranks the binding it shadows

| Step | Statement |
| --- | --- |
| Given | an invocation whose registry holds an object at a path |
| Given | an override bound at that path for this invocation |
| When | the path is resolved |
| Then | it answers the override |

## `SV-022` An override is how an unfilled path gets filled

| Step | Statement |
| --- | --- |
| Given | an invocation whose registry declares a path with no object |
| Given | an override bound at that path for this invocation |
| When | the path is resolved |
| Then | it answers the override |

## `SV-023` An override cannot introduce a name

| Step | Statement |
| --- | --- |
| Given | an invocation whose registry declares one path |
| When | an override is bound at a path the registry never declared |
| Then | `ArgumentError` is raised |

## `SV-024` What the override fills is what the guest reaches

| Step | Statement |
| --- | --- |
| Given | a Sandbox declaring a Service path with no object |
| When | guest code calls that path during an invocation whose block filled it |
| Then | the filled object's answer comes back |

## `SV-025` The override block serves the entrypoint verb too

| Step | Statement |
| --- | --- |
| Given | a Sandbox declaring a Service path with no object and carrying a preloaded entrypoint that calls it |
| When | the entrypoint runs during an invocation whose block filled that path |
| Then | the filled object's answer comes back |

## `SV-026` An override shadows a bound object as readily as an empty path

| Step | Statement |
| --- | --- |
| Given | a Sandbox with an object bound at a path |
| When | guest code calls that path during an invocation whose block bound another object there |
| Then | the block's object answers |

## `SV-027` An override is spent with its invocation

| Step | Statement |
| --- | --- |
| Given | a Sandbox with an object bound at a path |
| Given | an earlier invocation that overrode it |
| When | a later invocation with no block calls that path |
| Then | the bound object answers |

## `SV-028` A block that fills nothing changes nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox declaring a Service path with no object |
| When | guest code calls that path during an invocation whose block left it unfilled |
| Then | `Kobako::ServiceError` is raised |

## `SV-029` A Context is spent when its block returns

| Step | Statement |
| --- | --- |
| Given | a Context captured out of a completed invocation's block |
| When | an override is bound through it afterward |
| Then | `ArgumentError` is raised |
