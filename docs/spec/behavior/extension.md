# Extension

What installing a guest idiom with an optional host backend composes, and what stands behind its path.

## Why these scenarios

Installing an Extension is observable in two places: what the composition leaves behind — a snippet under one name, a Service under another — and what stands behind that path once an invocation begins, which the backend's declared keyword decides. The dependency scenarios cover the third: when that composition is checked, and how little the check asks.

Installing after the seal, an unmet dependency, and a malformed Extension all raise, so they belong to the error taxonomy and are specified there.

The `File` idiom the end-to-end witnesses install is an illustrative fixture — kobako ships no concrete Extension — so no scenario here states what that idiom does, only what installing one produces.

## EX-001 — The idiom becomes a snippet under the Extension's name

| Step | Statement |
| --- | --- |
| Given | an Extension named `File` carrying mruby source |
| When | `install` runs |
| Then | the snippet table holds one entry named `File` |

## EX-002 — The backend is bound where it says, not where the Extension is named

| Step | Statement |
| --- | --- |
| Given | an Extension named `File` whose backend declares the path `Vfs` |
| When | `install` runs |
| Then | the backend object is bound at `Vfs` |

## EX-003 — An idiom with nothing behind it binds nothing

| Step | Statement |
| --- | --- |
| Given | an Extension carrying source and no backend |
| When | `install` runs |
| Then | no Service is bound |

## EX-004 — Install chains

| Step | Statement |
| --- | --- |
| Given | an Extension carrying source |
| When | `install` runs |
| Then | it answers the registry it was called on |

## EX-005 — A method the idiom implements stays in the guest

| Step | Statement |
| --- | --- |
| Given | an installed Extension whose source defines a method needing no host |
| When | guest code calls that method |
| Then | it answers without a dispatch to the host |

## EX-006 — A method the idiom cannot answer reaches the backend

| Step | Statement |
| --- | --- |
| Given | an installed Extension whose backend is bound to a host object |
| When | guest code calls a method the idiom routes to that backend |
| Then | the host object's answer reaches the guest |

## EX-007 — An idiom is useful without a backend

| Step | Statement |
| --- | --- |
| Given | an installed Extension carrying source and no backend |
| When | guest code calls a method needing no host |
| Then | it answers |

## EX-008 — An unbacked idiom fails closed where it needs the host

| Step | Statement |
| --- | --- |
| Given | an installed Extension carrying source and no backend |
| When | guest code calls a method the idiom routes to a backend |
| Then | `Kobako::ServiceError` is raised |

## EX-009 — A backend declaring `object:` is bound at install

| Step | Statement |
| --- | --- |
| Given | an Extension whose backend declares `object:` |
| When | `install` runs |
| Then | that object is bound at the backend's path |

## EX-010 — A backend declaring `object:` takes no part in per-invocation resolution

| Step | Statement |
| --- | --- |
| Given | an installed Extension whose backend declares `object:` |
| When | an invocation resolves its backends |
| Then | the resolution carries no entry for that path |

## EX-011 — A backend declaring `provider:` holds its path before the first resolution

| Step | Statement |
| --- | --- |
| Given | an Extension whose backend declares `provider:` |
| When | `install` runs |
| Then | the path is bound to the `Kobako::Unresolved` sentinel |

## EX-012 — A backend declaring `provider:` yields a fresh object per invocation

| Step | Statement |
| --- | --- |
| Given | an installed Extension whose backend declares `provider:` |
| When | a second invocation resolves its backends |
| Then | the path carries a different object than the first resolution gave it |

## EX-013 — A backend declares one kind or the other

| Step | Statement |
| --- | --- |
| Given | a backend declaration carrying both `object:` and `provider:` |
| When | the backend is constructed |
| Then | `ArgumentError` is raised |

## EX-014 — A backend declaring neither keyword is a fillable

| Step | Statement |
| --- | --- |
| Given | an Extension whose backend declares neither `object:` nor `provider:` |
| When | `install` runs |
| Then | the path is bound to the `Kobako::Unresolved` sentinel |

## EX-015 — A fillable waits to be filled rather than resolved

| Step | Statement |
| --- | --- |
| Given | an installed Extension whose backend declares neither keyword |
| When | an invocation resolves its backends |
| Then | the resolution carries no entry for that path |

## EX-016 — One provider value is one resource

| Step | Statement |
| --- | --- |
| Given | two installed Extensions whose backends declare the same `provider:` value |
| When | an invocation resolves its backends |
| Then | the provider has been called once |

## EX-017 — A shared provider backs every path it was given to

| Step | Statement |
| --- | --- |
| Given | two installed Extensions whose backends declare the same `provider:` value |
| When | an invocation resolves its backends |
| Then | both paths carry the same object |

## EX-018 — Distinct providers are distinct resources

| Step | Statement |
| --- | --- |
| Given | two installed Extensions whose backends declare different `provider:` values |
| When | an invocation resolves its backends |
| Then | the two paths carry different objects |

## EX-019 — A provider's failure is the Host App's own

| Step | Statement |
| --- | --- |
| Given | an installed Extension whose `provider:` raises |
| When | an invocation resolves its backends |
| Then | the provider's own exception reaches the caller unwrapped |

## EX-020 — A provider's failure does not settle the Sandbox

| Step | Statement |
| --- | --- |
| Given | an installed Extension whose `provider:` raised on its first call and succeeds afterward |
| Given | an invocation that failed on that resolution |
| When | a later invocation resolves its backends |
| Then | the path carries the object the provider yielded |

## EX-021 — A fixed backend accumulates across invocations

| Step | Statement |
| --- | --- |
| Given | an installed Extension whose backend declares `object:` over a stateful host object |
| Given | an invocation that wrote to it through the guest idiom |
| When | a later invocation reads that value through the same idiom |
| Then | the written value comes back |

## EX-022 — A per-invocation backend accumulates nothing

| Step | Statement |
| --- | --- |
| Given | an installed Extension whose backend declares `provider:` over a stateful host object |
| Given | an invocation that wrote to it through the guest idiom |
| When | a later invocation reads that value through the same idiom |
| Then | the value is absent |

## EX-023 — A satisfied dependency passes the seal

| Step | Statement |
| --- | --- |
| Given | two installed Extensions, one declaring the other in `depends_on` |
| When | the registries seal |
| Then | the seal answers its registry |

## EX-024 — A dependency is matched as a name, not as a spelling

| Step | Statement |
| --- | --- |
| Given | two installed Extensions whose `depends_on` entry and name are written in different String and Symbol forms |
| When | the registries seal |
| Then | the seal answers its registry |

## EX-025 — Presence is all that is asserted, so cycles stand

| Step | Statement |
| --- | --- |
| Given | two installed Extensions each declaring the other in `depends_on` |
| When | the registries seal |
| Then | the seal answers its registry |

## EX-026 — The dependency assertion is a gate, not a recurring check

| Step | Statement |
| --- | --- |
| Given | a registry that has sealed once |
| Given | an Extension added afterward whose `depends_on` names nothing installed |
| When | the registries seal again |
| Then | the seal answers its registry |

## EX-027 — Every idiom is in place before any of them runs

| Step | Statement |
| --- | --- |
| Given | two installed Extensions, the dependent one installed first, whose source names the other's constant |
| When | guest code calls the dependent Extension's method |
| Then | it answers with the value read from the other Extension's constant |
