# SPEC.md — kobako

> This document is the authoritative specification for the kobako gem.
> It is written in progressive layers. This file currently contains the **Intent layer** only.
> Scope, Behavior, and Refinement layers will be appended in subsequent cycles.

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
<!-- Behavior layer: append after Scope -->
<!-- Refinement layer: append after Behavior -->
