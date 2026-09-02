# Sandbox

What a Sandbox is built with, what one invocation leaves for the next, and what a run hands back.

## Includes

- `test/unit/values/test_sandbox_options.rb`
- `test/unit/catalog/test_services.rb`
- `test/unit/catalog/test_snippets.rb`
- `test/e2e/sandbox/test_sandbox.rb`
- `test/e2e/sandbox/test_run.rb`
- `test/e2e/sandbox/test_preload.rb`
- `test/e2e/sandbox/test_usage.rb`
- `test/e2e/test_caps.rb`
- `test/e2e/test_lifecycle.rb`
- `test/e2e/test_preload.rb`
- `test/e2e/test_io_streams.rb`
- `test/e2e/test_execution.rb`
- `test/e2e/test_outcome_values.rb`
- `test/e2e/test_canonical_boot.rb`
- `test/parity/test_captures.rb`
- `test/parity/test_values.rb`
- `test/parity/test_isolation.rb`
- `test/parity/test_run_snippets.rb`
- `test/parity/test_caps_usage.rb`

### Why these scenarios

A Sandbox is set up once and run many times, so the scenarios fall into what construction fixes, what an invocation may not carry into the next, and what a run hands back. The isolation half is witnessed on both verbs and on their interleaving, because a mechanism that clears state on one entry and not the other passes either single-verb witness.

Captures are read after failures as often as after successes, so the trap paths carry their own scenarios rather than resting on the success ones. What a run wrote before it was cut short is exactly what a Host App triaging the failure has to read.

Everything that raises is a behavior too, settling on the class a Host App rescues and where the failure is attributed. The option checks belong to the runtime that performs them and are specified there; the entrypoint, snippet and preload refusals are here, each witnessed at whatever level shows it. What the registries do internally — how a name is normalized, what order entries keep — no public surface shows, so it is pinned by unit tests and is not a behavior.

The guest-side output surface — how `IO` and the Kernel writers behave inside the guest — belongs to the capability gem that implements it. What is here is the host end: which channel bytes land in, where they stop, and what survives a failure.

## `S-001` A Sandbox names the artifact it was built over

| Step | Statement |
| --- | --- |
| Given | no Sandbox |
| When | one is constructed with no `wasm_path:` |
| Then | it reports the bundled Guest Binary's path |

## `S-002` The caps a Sandbox was given are the caps it reports

| Step | Statement |
| --- | --- |
| Given | no Sandbox |
| When | one is constructed with each cap set |
| Then | each cap reader answers the value it was given |

## `S-003` The scheduling mode a Sandbox was given is the mode it reports

| Step | Statement |
| --- | --- |
| Given | no Sandbox |
| When | one is constructed with a scheduling mode |
| Then | its mode reader answers that mode |

## `S-004` Every cap has a value nobody has to supply

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying no caps |
| When | the caps are read |
| Then | each answers its default |

## `S-005` A cap set to nothing is a cap that does not bound

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying `nil` for each cap |
| When | the caps are read |
| Then | each answers `nil` |

## `S-006` A cap given a value keeps it

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying a value for each cap |
| When | the caps are read |
| Then | each answers the value it was given |

## `S-007` A whole-number deadline is still a deadline in seconds

| Step | Statement |
| --- | --- |
| Given | Sandbox options carrying an Integer `timeout:` |
| When | the timeout is read |
| Then | it answers the same quantity as a Float |

## `S-008` The memory budget is per invocation, not per Sandbox

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose memory cap is smaller than two allocations together |
| When | two invocations each allocate within the cap in turn |
| Then | both complete |

## `S-009` Disabling a cap is not requesting its default

| Step | Statement |
| --- | --- |
| Given | a Sandbox constructed with every cap set to `nil` |
| When | an invocation runs past what the defaults would have allowed |
| Then | it completes |

## `S-010` A deadline reached is not a Sandbox spent

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation was cut short by its deadline |
| When | a later invocation evaluates ordinary guest source |
| Then | it answers its value |

## `S-011` A memory budget reached is not a Sandbox spent either

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation was cut short by its memory budget |
| When | a later invocation evaluates ordinary guest source |
| Then | it answers its value |

## `S-012` Setup is paid once and serves every run

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service bound before its first invocation |
| When | several invocations in turn call that Service |
| Then | each reaches it |

## `S-013` What one evaluation writes to a guest global, the next cannot read

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose evaluation set a guest global |
| When | a later evaluation reads that global at entry |
| Then | it reads nothing |

## `S-014` The entrypoint verb leaks no more than the evaluation verb

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose entrypoint run set a guest global |
| When | a later entrypoint run reads that global at entry |
| Then | it reads nothing |

## `S-015` Interleaving the two verbs interleaves no state

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded entrypoint |
| When | evaluations and entrypoint runs alternate on it |
| Then | each observes only its own state |

## `S-016` Both frontends leave the same nothing behind

| Step | Statement |
| --- | --- |
| Given | a scenario whose first invocation defines a global, a constant and a reopened core method |
| When | both frontends run it and a second invocation reads all three |
| Then | they observe the same absences |

## `S-017` A failure is confined to the Sandbox that had it

| Step | Statement |
| --- | --- |
| Given | one Sandbox whose invocation failed |
| When | a separate Sandbox runs the same kind of work |
| Then | it completes |

## `S-018` The declared path set is fixed at the seal

| Step | Statement |
| --- | --- |
| Given | a registry sealed with a set of bound paths |
| Given | a path bound after the seal |
| When | the declared path set is read |
| Then | the later path is absent from it |

## `S-019` Before the seal the set still grows

| Step | Statement |
| --- | --- |
| Given | an unsealed registry |
| When | a path is bound and the declared path set is read |
| Then | the new path is present in it |

## `S-020` The set is ordered by when each path was bound

| Step | Statement |
| --- | --- |
| Given | a registry with several paths bound in turn |
| When | the declared path set is read |
| Then | the paths appear in the order they were bound |

## `S-021` A Sandbox that binds nothing declares nothing

| Step | Statement |
| --- | --- |
| Given | a registry with nothing bound |
| When | the declared path set is read |
| Then | it is empty |

## `S-022` A run that wrote nothing carries nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an invocation that writes to neither channel completes |
| Then | both captures are empty |

## `S-023` What was written and what was returned are read separately

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an invocation both writes output and returns a value |
| Then | the capture and the value are each readable in full |

## `S-024` Output past the cap is marked as cut, not silently short

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a standard-output cap |
| When | an invocation writes past it |
| Then | the standard-output truncation predicate is true |

## `S-025` A truncation mark belongs to the run that earned it

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation truncated its standard output |
| When | a later invocation writes within the cap |
| Then | its truncation predicate is false |

## `S-026` A trap does not carry its output into the next run

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation wrote output and then trapped |
| When | a later invocation writes its own output |
| Then | the capture holds only what the later invocation wrote |

## `S-027` Each run's capture is its own

| Step | Statement |
| --- | --- |
| Given | a Sandbox that has written output on an earlier invocation |
| When | a later invocation writes different output |
| Then | the capture holds only what the later invocation wrote |

## `S-028` The error channel is a channel of its own

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code writes to the error stream |
| Then | the bytes appear in the error capture |

## `S-029` A warning takes the error channel

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code issues a warning |
| Then | the bytes appear in the error capture |

## `S-030` The guest may point its output stream at the other channel

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose guest code pointed its output stream at the error stream |
| When | guest code writes to the output stream afterward |
| Then | the bytes appear in the error capture |

## `S-031` A redirected stream is redirected for one run only

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation pointed its output stream elsewhere |
| When | a later invocation writes to the output stream |
| Then | the bytes appear in the output capture |

## `S-032` The error channel is capped like the output channel

| Step | Statement |
| --- | --- |
| Given | a Sandbox with an error-output cap |
| When | an invocation writes past it |
| Then | the error truncation predicate is true |

## `S-033` An uncapped channel keeps what a capped one would have cut

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose standard-output cap is `nil` |
| When | an invocation writes past what the default cap would have allowed |
| Then | the capture holds every byte written |

## `S-034` Both frontends separate the two channels the same way

| Step | Statement |
| --- | --- |
| Given | a scenario writing to both channels |
| When | both frontends run it |
| Then | they observe the same two captures |

## `S-035` Both frontends cut at the same place

| Step | Statement |
| --- | --- |
| Given | a scenario writing past a channel's cap |
| When | both frontends run it |
| Then | they observe the same capture and truncation mark |

## `S-036` A deadline does not take back what was already written

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation wrote to standard output and then reached its deadline |
| When | the capture is read off the raised error's Execution |
| Then | it holds what was written before the deadline |

## `S-037` The error channel survives a deadline too

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation wrote to the error stream and then reached its deadline |
| When | the capture is read off the raised error's Execution |
| Then | it holds what was written before the deadline |

## `S-038` A truncation mark survives a deadline

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation truncated its output and then reached its deadline |
| When | the truncation predicate is read off the raised error's Execution |
| Then | it is true |

## `S-039` A memory budget does not take back what was written either

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation wrote to standard output and then exceeded its memory budget |
| When | the capture is read off the raised error's Execution |
| Then | it holds what was written before the budget was reached |

## `S-040` The last expression is the answer

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation whose last expression computes a value completes |
| Then | the invocation answers that value |

## `S-041` Both frontends carry the same values out of an evaluation

| Step | Statement |
| --- | --- |
| Given | a scenario evaluating each shape the wire carries |
| When | both frontends run it |
| Then | they observe the same values |

## `S-042` Both frontends carry the same values out of a Service

| Step | Statement |
| --- | --- |
| Given | a scenario whose Service returns each shape the wire carries |
| When | both frontends run it |
| Then | they observe the same values |

## `S-043` A value sent to the guest and returned comes back unchanged on both

| Step | Statement |
| --- | --- |
| Given | a scenario passing each shape through the guest and back |
| When | both frontends run it |
| Then | they observe the same values |

## `S-044` A value the wire cannot carry is refused the same way on both

| Step | Statement |
| --- | --- |
| Given | a scenario whose evaluation returns a value with no wire representation |
| When | both frontends run it |
| Then | they refuse it the same way |

## `S-045` An entrypoint is called and its answer comes back

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded entrypoint that takes no arguments |
| When | that entrypoint is run |
| Then | the invocation answers what the entrypoint returned |

## `S-046` Positional arguments reach the entrypoint

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded entrypoint that reads its positional arguments |
| When | that entrypoint is run with positional arguments |
| Then | the invocation answers a value computed from them |

## `S-047` Keyword arguments reach the entrypoint as a trailing Hash

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded entrypoint that reads a trailing Hash |
| When | that entrypoint is run with keyword arguments |
| Then | the invocation answers a value computed from them |

## `S-048` An entrypoint is named the same whether written as text or as a symbol

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded entrypoint |
| When | it is run with its name given as a String |
| Then | the invocation answers what the entrypoint returned |

## `S-049` Snippets are in place before the entrypoint is looked for

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose preloaded snippet defines the entrypoint |
| When | that entrypoint is run |
| Then | the invocation answers what it returned |

## `S-050` Both frontends run an entrypoint the same way

| Step | Statement |
| --- | --- |
| Given | a scenario running a preloaded entrypoint |
| When | both frontends run it |
| Then | they observe the same value |

## `S-051` Preloading chains

| Step | Statement |
| --- | --- |
| Given | a Sandbox before its first invocation |
| When | a snippet is preloaded onto it |
| Then | the preload answers the Sandbox |

## `S-052` A preloaded snippet is there when the guest looks

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded snippet that defines a constant |
| When | an evaluation names that constant |
| Then | it resolves |

## `S-053` A snippet replays for every invocation, not just the first

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded snippet that defines a constant |
| Given | one invocation already completed |
| When | a later evaluation names that constant |
| Then | it resolves |

## `S-054` Snippets replay in the order they were preloaded

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying several preloaded snippets whose effects depend on that order |
| When | an evaluation observes the result |
| Then | it reflects the order they were preloaded in |

## `S-055` Bytecode is a snippet as readily as source

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded bytecode snippet that defines a constant |
| When | an evaluation names that constant |
| Then | it resolves |

## `S-056` A bytecode snippet replays for every invocation too

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded bytecode snippet |
| Given | one invocation already completed |
| When | a later evaluation names the constant it defines |
| Then | it resolves |

## `S-057` Bytecode without a name still carries its effects

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying preloaded bytecode compiled without debug information |
| When | an evaluation names the constant it defines |
| Then | it resolves |

## `S-058` A run reports the time it spent

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation completes and its usage is read |
| Then | the wall time is above zero |

## `S-059` The entrypoint verb reports its time too

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying a preloaded entrypoint |
| When | that entrypoint runs and the usage is read |
| Then | the wall time is above zero |

## `S-060` A run reports the memory it took

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation that allocates completes and its usage is read |
| Then | the memory peak is above the no-allocation baseline |

## `S-061` A run cut short still reports what it spent

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation reached its deadline |
| When | the usage is read off the raised error's Execution |
| Then | the wall time is above zero |

## `S-062` The reported peak never exceeds the budget that refused it

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation exceeded its memory budget |
| When | the usage is read off the raised error's Execution |
| Then | the memory peak is at or below the configured budget |

## `S-069` A run the guest failed still reports what it spent

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation raised from guest code |
| When | the usage is read off the raised error's Execution |
| Then | the wall time is above zero |

## `S-070` A run a Service failed still reports what it spent

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation raised from a bound Service |
| When | the usage is read off the raised error's Execution |
| Then | the wall time is above zero |

## `S-063` Both frontends report usage after a run that succeeded

| Step | Statement |
| --- | --- |
| Given | a scenario whose invocation completes |
| When | both frontends run it and read the usage |
| Then | both carry a usage record |

## `S-071` A returned value keeps its zero bytes

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation returns a String carrying a zero byte |
| Then | the host receives every byte including the zero |

## `S-072` A raised message keeps its zero bytes too

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation raises with a message carrying a zero byte |
| Then | the raised error carries that message whole |

## `S-073` A value too deep or circular to encode fails cleanly

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation returns a value nested past the encoder's bound or referring to itself |
| Then | `Kobako::SandboxError` is raised rather than the invocation trapping |

## `S-074` Nesting within the bound crosses whole

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation returns a value nested to the encoder's bound |
| Then | the host receives the structure unchanged |

## `S-075` A Float crosses at full precision

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation returns a Float needing every bit of its payload |
| Then | the host receives the same Float |

## `S-076` An Integer crosses as an Integer

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation returns an Integer |
| Then | the host receives that Integer |

## `S-077` A guest Array arrives as an Array

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation returns an Array of mixed element types |
| Then | the host receives an Array with each element's type preserved |

## `S-078` A guest Hash arrives as a Hash, keys distinguished

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation returns a Hash keyed by both Symbols and Strings |
| Then | the host receives a Hash keeping that distinction |

## `S-079` An empty Array is an empty Array

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation returns an empty Array |
| Then | the host receives an empty Array |

## `S-080` An empty Hash is an empty Hash

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation returns an empty Hash |
| Then | the host receives an empty Hash |

## `S-064` A run hands back one frozen object carrying everything it produced

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation that writes output and returns a value completes |
| Then | the returned Execution is frozen and carries both |

## `S-065` A failed run hands the same object back through its error

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose invocation failed |
| When | the raised error's Execution is read |
| Then | it carries the observables the failed run produced |

## `S-066` A value that is nothing and a run that failed are told apart

| Step | Statement |
| --- | --- |
| Given | one Execution from a run whose value was legitimately nothing, and one from a failed run |
| When | each is asked whether it failed |
| Then | only the failed one says so |

## `S-067` A trapped run says it failed

| Step | Statement |
| --- | --- |
| Given | an Execution from a run cut short by a trap |
| When | it is asked whether it failed |
| Then | it says so |

## `S-068` A failure before any run produces no Execution to carry

| Step | Statement |
| --- | --- |
| Given | a Sandbox given an input refused before the guest runs |
| When | the raised error is asked for its Execution |
| Then | there is none |

## `S-081` An entrypoint that is there but cannot be called

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose preloaded snippet defines the entrypoint constant as a plain value |
| When | `#run` names that constant |
| Then | `Kobako::SandboxError` says it does not respond to the call |

## `S-082` Both frontends attribute an entrypoint fault the same way

| Step | Statement |
| --- | --- |
| Given | a scenario running an entrypoint that is missing and one that cannot be called |
| When | both frontends run it |
| Then | they observe the same failures |

## `S-083` A snippet that will not compile is still accepted

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | `#preload(code:)` is given source that does not compile |
| Then | the preload answers its Sandbox |

## `S-084` A snippet's compile failure surfaces on the invocation that replays it

| Step | Statement |
| --- | --- |
| Given | a Sandbox holding a preloaded snippet that does not compile |
| When | the first invocation runs |
| Then | `Kobako::SandboxError` carries the guest's syntax error |

## `S-085` A snippet name that is not a constant name

| Step | Statement |
| --- | --- |
| Given | a snippet table |
| When | a snippet is registered under a name that is not a constant name |
| Then | `ArgumentError` names the constraint |

## `S-086` A snippet name already taken

| Step | Statement |
| --- | --- |
| Given | a snippet table holding a snippet under a name |
| When | another snippet is registered under that same name |
| Then | `ArgumentError` says the name is already preloaded |

## `S-087` A preload after the snippet table is sealed

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose first invocation has begun |
| When | `#preload` runs |
| Then | `ArgumentError` names the first invocation |

## `S-088` A snippet that raises at replay is attributed to the snippet

| Step | Statement |
| --- | --- |
| Given | a Sandbox holding a preloaded snippet whose top-level expression raises |
| When | the first invocation runs |
| Then | the failure's backtrace names the snippet |

## `S-089` Bytecode that will not load is a structural failure

| Step | Statement |
| --- | --- |
| Given | a Sandbox holding preloaded bytecode whose body is corrupt |
| When | the first invocation runs |
| Then | `Kobako::BytecodeError` is raised |

## `S-090` Bytecode that loads and then raises is not one

| Step | Statement |
| --- | --- |
| Given | a Sandbox holding preloaded bytecode whose top-level expression raises |
| When | the first invocation runs |
| Then | the failure carries the guest's own exception class |

## `S-091` Both frontends attribute a snippet fault the same way

| Step | Statement |
| --- | --- |
| Given | a scenario preloading a snippet that will not compile and one that raises at replay |
| When | both frontends run it |
| Then | they observe the same failures |
