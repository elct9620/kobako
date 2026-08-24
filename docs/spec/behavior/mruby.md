# mruby guest

The state every invocation starts from, and what a raise inside a capability gem's frame costs.

## Why these scenarios

Two invocations of one artifact begin from the same interpreter state, not merely from a clean one. The heap-slot replay is what settles the difference: an interpreter that reset everything correctly but started somewhere new each time would pass every leak test and fail this one.

A capability gem servicing one guest operation calls back into guest code, and the raise that call may produce has to stay a guest exception. The two coercion entries are witnessed separately because they are separate frames, and the recovery scenario is what tells a guest error apart from a retired Sandbox.

That the boot state may be computed at build time, and that per-invocation resources may be provisioned ahead of demand, are both stated to be unobservable, so neither is a scenario. The reproducible-build check holds the baking end. Leak-freedom between invocations is per-invocation isolation and belongs with the Sandbox behaviors, whose witnesses cite it alongside this one.

## MR-001 — Two invocations begin from the same state, not merely a clean one

| Step | Statement |
| --- | --- |
| Given | a Sandbox that has evaluated `Object.new.object_id` once |
| When | the same source is evaluated again |
| Then | the same object id comes back |

## MR-002 — A constant an invocation defines is gone at the next entry

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose evaluation defined a constant |
| When | a later invocation asks whether that constant is defined |
| Then | it is not |

## MR-003 — A raise in an output coercion is the guest's error, not the Sandbox's

| Step | Statement |
| --- | --- |
| Given | guest source defining a class whose `to_s` raises |
| Given | an instance of it passed to `$stdout.puts` |
| When | the invocation runs |
| Then | `Kobako::SandboxError` is raised rather than `Kobako::TrapError` |

## MR-004 — The guest's own message survives the coercion frame

| Step | Statement |
| --- | --- |
| Given | guest source defining a class whose `to_s` raises |
| Given | an instance of it passed to `$stdout.puts` |
| When | the invocation runs |
| Then | the raised error carries the guest exception's message |

## MR-005 — The second coercion entry answers like the first

| Step | Statement |
| --- | --- |
| Given | guest source defining a class whose `inspect` raises |
| Given | an instance of it passed to `p` |
| When | the invocation runs |
| Then | `Kobako::SandboxError` is raised rather than `Kobako::TrapError` |

## MR-006 — The guest's own message survives the inspect frame too

| Step | Statement |
| --- | --- |
| Given | guest source defining a class whose `inspect` raises |
| Given | an instance of it passed to `p` |
| When | the invocation runs |
| Then | the raised error carries the guest exception's message |

## MR-007 — A raising callback costs the Sandbox nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation failed on a raising output coercion |
| When | a later invocation evaluates ordinary guest source |
| Then | it answers its value |
