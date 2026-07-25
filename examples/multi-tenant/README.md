# Multi-tenant — one Sandbox, one identity per invocation

A self-contained script that serves several tenants **concurrently from a
single shared `Kobako::Sandbox`**, with each invocation naming the tenant
it acts for. It is the concurrency companion to the
[serverless](../serverless/README.md) demo: that one gives each request
its own Sandbox, this one shows why it no longer has to.

## The shape, and why it has to be this shape

A Sandbox holds no state from any run — every invocation owns its Handles,
captures, and usage — so Threads may invoke one shared Sandbox
concurrently (SPEC B-22). What each invocation still needs is its own
identity, and that is the split between the two binding forms:

```
setup   sandbox.bind("Audit", audit)     ->  static: one object, every Thread
        sandbox.bind("Tenant::Store")    ->  fillable: no object yet

per run sandbox.eval(source) { |ctx| ctx.bind("Tenant::Store", store) }
```

`bind(path)` with no object declares a **fillable** path: the guest sees
the constant while the host defers the object it stands for, and an
invocation that never fills it is refused as a `Kobako::ServiceError`
rather than served a stale one (B-62). The `#eval` / `#run` block fills it
for that one invocation through `ctx.bind` (B-63).

The guest source is identical for every tenant and names none of them.
Which ledger a run reaches is entirely the host's decision, made per
invocation — so a tenant's data is never reachable from another tenant's
run, and adding a tenant needs no new guest code.

## The obligation sharing adds

One thing does change when Threads share a Sandbox: a **statically bound
object is reached by every Thread at once and must be thread-safe
itself**. kobako isolates invocations from each other, not a host object
from itself. `Audit` here guards its own array with a `Mutex`; the
per-invocation `Store` needs nothing, because no other Thread can see it.

That asymmetry is the practical reason to prefer `ctx.bind` (or an
Extension `provider:`) for anything mutable, and to keep statically bound
Services immutable or explicitly synchronised.

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on
first run — no `Gemfile` is required in the working directory.

```bash
ruby examples/multi-tenant/app.rb
```

From a clone of the kobako repository, prefix with `bundle exec` so the
local checkout is used instead of the released gem.

## What to observe

```
$ ruby examples/multi-tenant/app.rb
multi-tenant demo — one Sandbox, one identity per invocation

concurrent runs on the SHARED Sandbox (3 Threads, gvl: :release):
  acme     total 9650    own ledger only
           stdout "acme: 3 entries\n"
  globex   total 198     own ledger only
           stdout "globex: 2 entries\n"
  initech  total 53550   own ledger only
           stdout "initech: 4 entries\n"

unfilled invocation — nothing bound for this run:
  Tenant::Store.ledger : refused   # fails closed (Kobako::Unresolved)

statically bound Audit — one object, every Thread:
  recorded ["acme", "globex", "initech"]   # its own Mutex, not kobako's isolation
```

Three things to read off the trace. Each total matches only its own
tenant's ledger, so the `Ledger` Handle each run minted stayed inside that
run. Each Execution carries its own `stdout` — one line naming one tenant
— even though three runs printed concurrently through the same Sandbox.
And the unfilled invocation is refused instead of falling back to whatever
the previous run bound.

## About `gvl:`

The Sandbox is constructed with `gvl: :release`, which drops Ruby's GVL
for the guest span so the three Threads run their guest code in parallel
(B-64). It changes scheduling only — every line this demo prints is
identical under the default `:hold`. Releasing pays a handoff cost at
every guest→host dispatch, so it earns its keep on compute-heavy guest
work and can cost more than it saves on dispatch-heavy work;
`rake bench:gvl_scheduling` measures both ends.
