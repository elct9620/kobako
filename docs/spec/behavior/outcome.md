# Outcome attribution

What one invocation's result settles into, which side it is attributed to, and which class a Host App rescues it as.

## Includes

- `test/unit/outcome/test_attribution.rb`
- `test/unit/outcome/test_decoding.rb`
- `test/unit/outcome/test_value_sharing.rb`
- `test/unit/values/test_error_class_hierarchy.rb`
- `test/parity/test_errors.rb`
- `crates/kobako-wasmtime/src/invocation.rs`
- `crates/kobako-wasmtime/src/trap.rs`
- `crates/kobako/src/execution.rs`
- `crates/kobako/src/error.rs`
- `crates/kobako/src/msgpack/execution.rs`
- `wasm/kobako-mruby/src/flows/boot.rs`

### Why these scenarios

Every invocation writes exactly one Outcome, so what a Host App can do about a failure is decided entirely here: whether the Sandbox is still usable, whose mistake it was, and which class the rescue has to name. The scenarios follow that decision in the order it is made — the arm, then the origin, then the class the guest named.

The origin decides the layer and the class name may only narrow inside it. Both halves are witnessed, because attribution that let a guest-chosen name cross a layer would hand the guest its own error taxonomy.

Two arms carry no record at all — nothing written, and bytes the envelope cannot frame — and both mean the guest runtime is past reasoning about. They are separated from every readable failure because they are the only ones that cost the Sandbox.

The class hierarchy is asserted as relations rather than through failures. A Host App writes one rescue and expects it to cover a family; what makes that true is the shape of the tree, which no single failure shows.

The parity scenarios settle that both frontends attribute the same origin, not that either is right — what is right is stated by the scenarios above.

One block here is deliberately written in one frontend's own names: the ancestry scenarios say what the Ruby gem's error classes descend from, which is a promise that frontend makes and no other can answer for. Everything else states the attribution itself, so either frontend's tests witness it in its own spelling.

## `OC-001` A guest that wrote nothing costs the Sandbox

| Step | Statement |
| --- | --- |
| Given | an invocation whose guest wrote no result |
| When | the outcome is read |
| Then | it fails as a trap saying the Sandbox exited without producing one |

## `OC-002` So does a result the envelope cannot frame

| Step | Statement |
| --- | --- |
| Given | an invocation whose result bytes the envelope cannot frame |
| When | the outcome is read |
| Then | it fails as a trap, there being nothing left to attribute to |

## `OC-003` A result the codec cannot read does not

| Step | Statement |
| --- | --- |
| Given | an invocation answering bytes the payload codec cannot read |
| When | the outcome is read |
| Then | it fails as a Sandbox failure rather than a trap |

## `OC-004` The refusal stays in the caller's vocabulary

| Step | Statement |
| --- | --- |
| Given | an invocation answering a payload the codec cannot read |
| When | the Host App reads the failure's message |
| Then | it names an invalid result and no codec detail |

## `OC-005` The codec's own account is still reachable

| Step | Statement |
| --- | --- |
| Given | an invocation answering a payload the codec cannot read |
| When | the failure is asked for its detailed form |
| Then | it carries more than the message did |

## `OC-006` A result the codec can read is the invocation's value

| Step | Statement |
| --- | --- |
| Given | an invocation answering an encoded value |
| When | the outcome is read |
| Then | it answers that value |

## `OC-007` A large value is read without copying its bytes

| Step | Statement |
| --- | --- |
| Given | an invocation answering a value larger than a copy would be free for |
| When | the outcome is read |
| Then | the value holds less memory than its own bytes would occupy |

## `OC-008` A failure the Service caused is attributed to the Service

| Step | Statement |
| --- | --- |
| Given | a failed invocation whose record names the service origin |
| When | the outcome is read |
| Then | it fails as a Service failure carrying that origin |

## `OC-009` A failure the sandbox caused is attributed to the sandbox

| Step | Statement |
| --- | --- |
| Given | a failed invocation whose record names the sandbox origin |
| When | the outcome is read |
| Then | it fails as a Sandbox failure and not as a Service failure |

## `OC-010` An origin the contract does not name lands with the sandbox

| Step | Statement |
| --- | --- |
| Given | a failed invocation whose record names an origin the contract reserves nothing for |
| When | the outcome is read |
| Then | it fails as a Sandbox failure carrying that origin unchanged |

## `OC-011` A class the guest named narrows inside the origin's branch

| Step | Statement |
| --- | --- |
| Given | failed invocations whose records name a class within their own origin's branch |
| When | each outcome is read |
| Then | each settles as the class it named |

## `OC-012` A class outside that branch is ignored

| Step | Statement |
| --- | --- |
| Given | a failed invocation whose record names a class belonging to another origin's branch |
| When | the outcome is read |
| Then | it settles as the origin's own class |

## `OC-013` A bytecode failure is rescuable on its own

| Step | Statement |
| --- | --- |
| Given | a failed invocation whose record names the bytecode class |
| When | the outcome is read |
| Then | it fails as a bytecode failure and is still a Sandbox failure |

## `OC-014` An unresolved entrypoint carries its correction

| Step | Statement |
| --- | --- |
| Given | a failed invocation whose record names an entrypoint that did not resolve |
| When | the outcome is read |
| Then | the failure carries the name asked for and the names available |

## `OC-015` A failed arm never asks the codec anything

| Step | Statement |
| --- | --- |
| Given | a failed invocation carrying unreadable bytes in its value slot |
| When | the outcome is read |
| Then | the failure names itself from its record rather than reporting a wire failure |

## `OC-016` One rescue covers every invocation outcome

| Step | Statement |
| --- | --- |
| Given | the three classes an invocation can settle into |
| When | their ancestry is read |
| Then | each descends from `Kobako::Error` |

## `OC-017` Construction failures are their own branch

| Step | Statement |
| --- | --- |
| Given | `Kobako::SetupError` |
| When | its ancestry is read |
| Then | it descends from `Kobako::Error` without being an invocation outcome |

## `OC-018` Running out of references is a Sandbox failure

| Step | Statement |
| --- | --- |
| Given | the reference-exhaustion class |
| When | its ancestry is read |
| Then | it descends from `Kobako::SandboxError` |

## `OC-019` So is an entrypoint that did not resolve

| Step | Statement |
| --- | --- |
| Given | `Kobako::UndefinedEntrypointError` |
| When | its ancestry is read |
| Then | it descends from `Kobako::SandboxError` |

## `OC-020` Every dispatch failure is a Service failure

| Step | Statement |
| --- | --- |
| Given | each class a dispatch can fail as |
| When | their ancestry is read |
| Then | each descends from `Kobako::ServiceError` |

## `OC-021` A yield-site failure is not an invocation outcome

| Step | Statement |
| --- | --- |
| Given | each class a yield site can fail as |
| When | their ancestry is read |
| Then | each descends from `Kobako::Error` and from none of the outcome classes |

## `OC-022` A deadline reached is a trap

| Step | Statement |
| --- | --- |
| Given | `Kobako::TimeoutError` |
| When | its ancestry is read |
| Then | it descends from `Kobako::TrapError` |

## `OC-023` So is a memory budget reached

| Step | Statement |
| --- | --- |
| Given | `Kobako::MemoryLimitError` |
| When | its ancestry is read |
| Then | it descends from `Kobako::TrapError` |

## `OC-024` Both frontends attribute an uncaught guest exception the same way

| Step | Statement |
| --- | --- |
| Given | a scenario whose guest code raises, once anonymously and once as its own class |
| When | both frontends run it |
| Then | they observe the same failures |

## `OC-025` Both attribute a source that will not compile the same way

| Step | Statement |
| --- | --- |
| Given | a scenario evaluating source that does not compile |
| When | both frontends run it |
| Then | they observe the same failure |

## `OC-026` Both interrupt a runaway invocation at the same cap

| Step | Statement |
| --- | --- |
| Given | a scenario looping without end under a deadline |
| When | both frontends run it |
| Then | they observe the same failure |

## `OC-027` And both stop a runaway allocation at the same cap

| Step | Statement |
| --- | --- |
| Given | a scenario allocating past a memory budget |
| When | both frontends run it |
| Then | they observe the same failure |

## `OC-028` A failure neither cap caused is still a trap

| Step | Statement |
| --- | --- |
| Given | a guest failure attributable to neither cap |
| When | it is read |
| Then | it is a trap of no named cap |

## `OC-029` A trap carries its reason, not only the frames around it

| Step | Statement |
| --- | --- |
| Given | a trap whose reason sits beneath the frames describing where it happened |
| When | its message is read |
| Then | the reason and the frames are both there |

## `OC-030` A trap carrying no frames is not repeated

| Step | Statement |
| --- | --- |
| Given | a trap carrying a reason and no frames |
| When | its message is read |
| Then | the reason appears once |

## `OC-031` A construction failure reads as the message it was given

| Step | Statement |
| --- | --- |
| Given | a construction failure carrying a message |
| When | it is read as text |
| Then | it is that message and nothing around it |

## `OC-032` A deadline reached is classified as the deadline's own trap

| Step | Statement |
| --- | --- |
| Given | a run cut short by its deadline |
| When | the failure is classified |
| Then | it is the deadline's own kind, carrying the message it was given |

## `OC-033` A memory budget reached is classified as the budget's own trap

| Step | Statement |
| --- | --- |
| Given | a run cut short by its memory budget |
| When | the failure is classified |
| Then | it is the budget's own kind, carrying the message it was given |

## `OC-034` Each trap kind reaches the Host App as its own failure

| Step | Statement |
| --- | --- |
| Given | a trap of each kind |
| When | each is carried out to the Host App |
| Then | each arrives as the failure kind naming what stopped the run |

## `OC-041` A construction failure never becomes an invocation outcome

| Step | Statement |
| --- | --- |
| Given | a construction failure |
| When | it is carried out to the Host App |
| Then | it stays a construction failure rather than becoming an outcome |

## `OC-043` A failure before the guest is ready is the Sandbox's own boot failure

| Step | Statement |
| --- | --- |
| Given | a guest that could not read what it was set up with |
| When | the Outcome is written |
| Then | it attributes to the Sandbox under the boot failure's own name |

## `OC-044` A failure reading the invocation is the Sandbox's own wire failure

| Step | Statement |
| --- | --- |
| Given | a guest that could not read the invocation it was handed |
| When | the Outcome is written |
| Then | it attributes to the Sandbox under the wire failure's own name |

## `OC-045` Every class a failed Service call raises attributes to the Service

| Step | Statement |
| --- | --- |
| Given | each class a failed Service call raises |
| When | the origin is decided |
| Then | each attributes to the Service |

## `OC-046` Every other class attributes to the Sandbox

| Step | Statement |
| --- | --- |
| Given | a class no failed Service call raises |
| When | the origin is decided |
| Then | it attributes to the Sandbox |

## `OC-047` A guest class named like one of kobako's claims no Service origin

| Step | Statement |
| --- | --- |
| Given | a class the guest defined whose name resembles one kobako raises |
| When | the origin is decided |
| Then | it attributes to the Sandbox |
