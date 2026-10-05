# Intent

kobako gives a Ruby application an in-process sandbox, quick to start, for
running mruby code it does not trust. It is to Ruby what a V8 isolate is to
JavaScript: embeddable, self-hosted, and isolated from the process around it.

```
Host App --eval / run--> Sandbox --> Guest Binary (mruby in Wasm)
    ^                                      |
    +------------ Service calls <----------+
```

## Users

Each user runs code they did not write, inside a process they already run.

| User | Goal |
|------|------|
| Host App developer (Rails, Rack, Sidekiq, CLI) | Run third-party Ruby logic without risking the process or its data |
| LLM agent framework author | Run model-generated code and read back structured results |
| Teaching or CI operator | Evaluate submitted scripts without provisioning containers |
| No-code tool builder | Evaluate formula fields and filter rules safely |

## Impacts

Each impact is a change a Host App can observe, and none lets guest code reach
host memory, I/O, or credentials. The scenarios under
[`spec/behavior/`](spec/behavior/) state each one in full.

| Impact | What the Host App gains |
|--------|-------------------------|
| One-shot execution | `#eval` answers a result or a categorized error |
| Preload and dispatch | preload source or bytecode once, dispatch many `#run` calls |
| Service injection | Services are guest code's only reach outside the Sandbox |
| Block yield | a Service method yields to a guest block, still isolated |
| Error attribution | each failure is a trap, a Sandbox failure, or a Service failure |
| Output capture | guest stdout and stderr, apart from the Transport channel |
| Extension installation | `#install` gives guest code a native-style constant |
| Host-parallel execution | `gvl: :release` runs Sandboxes in parallel, changing scheduling only |

## Non-Goals

kobako stays a sandbox library. What surrounds running one belongs to the Host
App or to other projects.

| Left out | Kept in scope instead |
|----------|-----------------------|
| LLM integration, agent frameworks, prompt engineering | — |
| A general-purpose wasmtime Ruby gem | — |
| mruby upstream development or distribution | — |
| Billing, SLAs, deployment and operations tooling | — |
| Cross-Sandbox quotas, fairness, and aggregate metrics | per-invocation caps and usage |
| Fair ordering among waiting Pool checkouts | the bounded Pool checkout |
| Adaptive scheduling, or observing it per invocation | the per-Sandbox `gvl:` mode |
| Async or resumable execution, interpreter snapshots | — |
