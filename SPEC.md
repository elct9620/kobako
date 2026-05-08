# SPEC.md — kobako

> This document is the authoritative specification for the kobako gem.
> It is written in progressive layers. This file currently contains the **Intent** and **Scope** layers.
> Behavior and Refinement layers will be appended in subsequent cycles.

---

## Intent

### Purpose

kobako provides Ruby applications with an in-process, low-cold-start sandbox for executing untrusted mruby code, filling a gap in the Ruby ecosystem where no embeddable, self-hostable isolation runtime exists comparable to V8 isolates in JavaScript.

### Users

| User | Goal |
|------|------|
| Host App developer (Rails / Rack / Sidekiq / CLI) | Execute untrusted or third-party Ruby logic inside their application without risking process integrity or data leakage |
| LLM agent framework author | Run dynamically generated Ruby code produced by a model in a safe, in-process environment and retrieve structured results |
| Teaching platform / CI system operator | Evaluate user-submitted Ruby scripts in isolation without provisioning full containers |
| No-code / low-code tool builder | Evaluate untrusted Ruby expressions (e.g., formula fields, webhook filter rules) safely within their platform |

### Impacts

When kobako is working correctly:

- A Host App can execute arbitrary mruby code supplied at runtime and receive a structured result or a categorized error — without any guest code affecting host memory, I/O, or credentials.
- A Host App can inject named Ruby service objects that guest code can call via RPC; those objects are the only mechanism by which guest code can interact with external resources.
- Errors produced during guest execution are attributable to one of three distinct origins (Wasm trap, sandbox/wire fault, or guest application error), enabling the Host App to handle each case differently.
- Guest stdout and stderr are captured and exposed separately from the RPC protocol channel, allowing Host Apps to surface guest logs without confusing them with protocol messages.

### Non-Goals

The following are explicitly outside the scope of kobako:

- LLM integration, agent frameworks, or prompt engineering (kobako provides the execution substrate; connecting it to an LLM is the Host App's responsibility)
- A general-purpose wasmtime Ruby gem — `ext/` is a private binding, not a reusable wasmtime wrapper
- mruby upstream development or distribution — kobako consumes a pinned mruby release tarball
- Multi-tenant billing, SLA management, or deployment/operations tooling
- Async / yield-resume execution models or interpreter state snapshot/resume (kobako uses synchronous blocking RPC; Monty-style `start()`/`resume()` and `dump()`/`load_snapshot()` are not provided)
- Passing guest-side blocks to Service methods — guest closures cannot be executed on the host side; iteration is handled by returning collections from Service methods

### Core Abstractions

These five roles describe the system. All design and behavior content in later layers uses these names exclusively.

| Role | Responsibility | Scope |
|------|---------------|-------|
| **Host App** | The Ruby application (Rails / Rack / CLI) that uses kobako; holds all credentials and policy | Out of scope — must be named but not designed here |
| **Host Gem** | The kobako gem itself: Ruby API layer (`lib/`) + private native extension (`ext/`); exposes the sandbox interface, routes RPC, and manages Handle lifecycle | In scope |
| **Guest Binary** | `kobako.wasm` — compiled from the `wasm/` Rust crate; contains the mruby interpreter and RPC client; is the isolation boundary | In scope |
| **Service** | A Host Ruby object injected into the sandbox under a two-level name (`Group::Member`); the only mechanism by which guest code can access host resources | In scope |
| **Wire Spec** | The MessagePack contract governing all host↔guest RPC messages; not a runtime object but a shared protocol both sides implement | In scope |

**Key internal concepts** (refined in later layers):

- **Sandbox** (`Kobako::Sandbox`): the runtime unit that instantiates the Guest Binary, injects Services, executes a mruby script, and returns a structured outcome or raises a typed error.
- **Registry**: the Host Gem sub-component that maintains Service Group / Member registrations, routes incoming RPC calls to the correct host object, and owns the HandleTable.
- **Handle**: an opaque integer token the guest holds to reference a host-side object returned by a Service call; the guest can use it in subsequent RPC calls but cannot dereference it directly.
- **HandleTable**: the host-side mapping from Handle IDs to Ruby objects; owned by Registry and not exposed to Host App.
- **Service Group / Member**: `Group` is a declared namespace visible to guest as a Ruby module; `Member` is a named binding within the group visible to guest as a module constant.
- **Three-layer error taxonomy**: `Kobako::TrapError` (Wasm trap), `Kobako::SandboxError` (wire or runtime fault), `Kobako::ServiceError` (guest application error) — each with distinct attribution and handling semantics.

---

<!-- Scope layer: append here -->

---

## Scope

> This file currently contains the **Intent layer** and **Scope layer**.
> Behavior and Refinement layers will be appended in subsequent cycles.

### IS / IS-NOT

#### kobako IS

**Gem deliverable**
- A single Ruby gem (`kobako`) that ships one published artifact: a source-only gem containing `lib/`, `ext/`, and `data/kobako.wasm`.
- The gem is distributed source-only; users compile the native extension (`ext/`) via `cargo` on their host machine. No precompiled native extension is included.

**Host Gem — Ruby API surface**
- `Kobako::Sandbox` — the single entry point for Host App developers: instantiate, inject Services, run a mruby script, receive a structured result or a typed error.
- `Kobako::Sandbox#define(name)` — declares a Service Group by name; returns a `Kobako::Service::Group`.
- `Kobako::Service::Group#bind(name, object)` — binds a Host Ruby object as a named Service Member within the group.
- `Kobako::Sandbox#run(script)` — executes a mruby script synchronously; returns the deserialized value of the script's last expression.
- `Kobako::Sandbox#stdout` / `#stderr` — exposes captured guest output as separate, protocol-free channels.
- Three typed error classes raised by `#run`: `Kobako::TrapError`, `Kobako::SandboxError`, `Kobako::ServiceError`.
- `Kobako::Handle` — an opaque token exposed to the Host App when a Service method returns a stateful host-side object; usable in subsequent RPC calls from guest code.

**Host Gem — internal components (in scope to specify, not exposed to Host App)**
- `Registry` — maintains Service Group / Member registrations, routes RPC calls, owns the HandleTable.
- `HandleTable` — maps opaque Handle IDs to Ruby objects; lifecycle managed entirely by the Host Gem.
- `lib/kobako/wire.rb` — encodes and decodes MessagePack ext types (0x01 Capability Handle, 0x02 Exception envelope); not exposed to Host App.

**Host Gem — native extension (`ext/`)**
- A private Rust crate that uses the `wasmtime` crate as its Wasm runtime and is bridged to Ruby via `magnus` / `rb-sys`.
- Exposes to Ruby exactly: `Kobako::Sandbox`, `Kobako::Handle`, the three error classes, and `Kobako::HandleTableExhausted` (a `SandboxError` subclass).
- Capability state is isolated between successive `#run` invocations of the same `Sandbox`.
- Guest stdin, stdout, and stderr are bound to in-process buffers; stdout and stderr are never used as protocol channels.

**Guest Binary (`kobako.wasm`)**
- A `wasm32-wasip1` binary produced by the `wasm/` Rust crate, containing the mruby interpreter (fixed component, not a replaceable role) and the RPC client.
- Shipped inside `data/kobako.wasm` and loaded by `lib/` at runtime; not embedded in the native extension binary.
- Accepts a mruby script as its execution input and produces exactly one outcome per execution: either the script's last expression value or an uncaught exception with origin attribution.

**Wire Spec (the MessagePack RPC contract)**
- Defines the message structure both sides implement: Request (target, method, args), Response (result or error variant), Result envelope (user script return value), and Panic envelope (uncaught exception with origin attribution).
- Defines two MessagePack ext codes: `0x01` (Capability Handle), `0x02` (Exception envelope).
- Defines the Wire ABI: host import function `__kobako_rpc_call`; guest exports `__kobako_run`, `__kobako_alloc`, `__kobako_take_outcome`.
- Guest is the sole initiator; host responds synchronously within the same Wasm call frame. Host never pushes unsolicited messages.
- The Wire Spec is a release-internal contract; it carries no in-band version field.

**Service injection model**
- Services are registered under a two-level namespace: `Group::Member` (e.g., `MyService::KV`).
- Guest code calls `Group::Member.method(args)`; the Host Gem routes the call to the bound Ruby object and returns the serialized result.
- When a Service method returns a stateful host-side object, the wire layer automatically registers it in the HandleTable and encodes it as a Capability Handle (ext `0x01`). The guest holds the Handle ID and may pass it in subsequent RPC calls; it cannot dereference the object directly.

**Three-layer error attribution**
- `Kobako::TrapError` — Wasm trap (runtime crash at the Wasm engine level).
- `Kobako::SandboxError` — wire or runtime fault (malformed payload, HandleTable exhaustion, instantiation failure).
- `Kobako::ServiceError` — guest application error (uncaught mruby exception attributable to the guest script or a Service call result).
- Attribution is determined by a two-step decision: trap detection first; then outcome envelope tag (`result` vs `panic`) for non-trap outcomes.

**Build and quality pipeline**
- `bundle exec rake compile` is the single command to produce a working `ext/` native extension and `data/kobako.wasm` from a clean clone.
- Vendor dependencies (mruby release tarball, wasi-sdk) are fetched by rake tasks; they are not committed to the repository.
- Four test layers: Codec round-trip fuzz, Wire integration tests, Ext unit tests, End-to-end tests.
- Five benchmarks are maintained as a regression baseline; results are committed to the repository.
- Release gating requires all four test layers green, build pipeline guard passing, benchmarks run, and the six release documentation artifacts present: README, this development guide, wire spec, build guide, CHANGELOG, and LICENSE.

**Platform support**
- Linux and macOS only.

---

#### kobako IS-NOT

**Not part of the gem's public API or in-scope design**
- LLM integration, agent frameworks, or prompt engineering — kobako provides the execution substrate only; connecting it to an LLM is the Host App's responsibility.
- A general-purpose wasmtime Ruby gem — `ext/` is a private binding; `Engine`, `Store`, `Linker`, `Module`, and other wasmtime types are not exposed to the Host App or to downstream gems.
- mruby upstream development or distribution — kobako consumes a pinned mruby release tarball and does not contribute to or republish mruby.
- Multi-tenant billing, SLA management, deployment, or operational tooling.

**Execution model exclusions**
- Async / yield-resume execution — kobako executes scripts synchronously (blocking RPC). Monty-style `start()` / `resume()` and `dump()` / `load_snapshot()` are not provided.
- Guest-side blocks passed to Service methods — guest closures cannot be executed on the host side; the wire layer has no encoding for them. Iteration patterns are handled by returning collections from Service methods.
- Iterator Handles — there is no handle type for host-side iterators; guest iterates over returned Arrays using standard mruby methods.
- Host-initiated message push — the host never sends unsolicited messages to the guest; all communication is guest-initiated.

**Distribution and platform exclusions**
- Precompiled native extensions — kobako is source-only; no precompiled gem variant is provided.
- Windows support — kobako targets Linux and macOS only; CI does not include a Windows matrix.

**Design boundary: what kobako does not constrain**
- Which methods a Service object exposes — that is Host App policy; kobako only defines the injection API.
- How the Host App handles errors — kobako raises typed errors; the Host App decides what to do with each class.
- How the Host App connects to external resources — all external access is mediated through Service objects that the Host App defines and injects.

---

### Feature List

The following features constitute the complete surface of the `kobako` gem. Behavior details for each feature are specified in the Behavior layer.

| # | Feature | Role |
|---|---------|------|
| F-01 | **Sandbox lifecycle** — create, run, and tear down an isolated mruby execution environment per invocation | Host Gem |
| F-02 | **Service Group declaration** — register a named two-level namespace (`Group::Member`) into a Sandbox before execution | Host Gem |
| F-03 | **Service Member binding** — attach a Host Ruby object to a declared Group slot; the object becomes callable from guest code | Host Gem |
| F-04 | **Script execution** — run a mruby script string synchronously inside the Sandbox and return the script's last expression as a deserialized Ruby value | Host Gem + Guest Binary |
| F-05 | **RPC dispatch** — route a guest-initiated call (`Group::Member.method(args)`) to the correct Host Ruby object and return the serialized result to the guest | Host Gem + Wire Spec |
| F-06 | **Capability Handle** — when a Service method returns a stateful host-side object, encode it as an opaque Handle (ext `0x01`); the guest may use the Handle in subsequent RPC calls | Host Gem + Wire Spec |
| F-07 | **Three-layer error attribution** — classify every execution failure as `TrapError`, `SandboxError`, or `ServiceError` and raise the appropriate typed exception to the Host App | Host Gem |
| F-08 | **Guest stdout / stderr capture** — buffer all guest `puts` / `$stderr.write` output and expose it via `Sandbox#stdout` / `#stderr` after execution, separated from the RPC protocol channel | Host Gem + Guest Binary |
| F-09 | **Wire codec** — encode and decode all host↔guest messages using MessagePack with two registered ext types (Handle `0x01`, Exception `0x02`) | Wire Spec (both sides) |
| F-10 | **Build pipeline** — produce `ext/` native extension and `data/kobako.wasm` reproducibly from a clean clone via `bundle exec rake compile` | Build tooling |
| F-11 | **Test suite** — four-layer test coverage (Codec fuzz, Wire integration, Ext unit, E2E) with five regression benchmarks | Quality pipeline |

<!-- Behavior layer: append after Scope -->
<!-- Refinement layer: append after Behavior -->
