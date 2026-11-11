# Conventions

How code in kobako is judged while it is written: which answer a behavior gives, what a contract registers, and how a name is worded. [`architecture.md`](../architecture.md) says where code goes and orders the baselines; this applies them. Every rule applies to Rust, Ruby, and RBS alike.

| Question | Section |
|---|---|
| which answer a behavior gives | Behavior Judgment |
| what a difference from a reference owes | Forced Differences |
| how an error crosses the layers | Errors |
| what a contract registers | Contract Registration |
| what a name must achieve | Naming Principles |
| what a name or a comment may point at | Self-Contained Wording |
| how a name is settled | Naming Precedence |
| which word class names what | Word Classes, Method Names |
| where `-ed` and `-ing` stand | Participles |
| how a question, a raise, or a limit reads | Questions, Raising Methods, Limits |
| when an adjective stands alone | Kinds and Markers |
| how files and tests are named | Files and Tests |
| which names come from elsewhere | Fixed Names |

## Behavior Judgment

An answer is settled by these steps, in the order Baselines in [`architecture.md`](../architecture.md) sets; the first that decides it ends the search.

1. If security refuses the case, refuse it, with the error the reference gives in the nearest case.
2. If the WASI contract cannot carry the answer, give what it can carry.
3. Otherwise take the layer's reference: Linux inside `kobako-runtime`, WIT then wasmtime-wasi at the boundary, CRuby for the guest's Ruby.
4. If step 1 or 2 decided, write the difference into a scenario.

Observe a reference rather than recall it. Run Linux and CRuby in a container, and read wasmtime-wasi at its pinned version.

### Forced Differences

Each departure from a reference names the step that forced it, so a reader can tell a limit from a mistake.

| Forced by | The scenario says | As in |
|---|---|---|
| security | kobako refuses where the reference answers | a UDP read with no peer, `NW-181` |
| the WASI contract | what WASI carries no room for | a send flag, `NW-176` |
| an answer that turns on timing | the one fixed answer kobako gives | a `Net::HTTP` timeout, `NW-177` |

A departure only refuses more, never succeeds differently. Its scenario is the only record of it, so the scenario says what the reference would answer.

### Errors

An error is decided once and translated at each boundary, so no layer invents its own.

```
kobako-runtime   Linux decides the errno
     |
WASI boundary    WIT maps it to an error code; preview1's must pass wasi-testsuite
     |
guest Ruby       the Errno CRuby raises for it, worded as mruby words it
```

## Contract Registration

A contract is a promise to someone outside kobako: changing it breaks them. It registers only what is promised.

| Registers | Leaves out |
|---|---|
| a seam a third party implements, with every method | a name only kobako's own crates call |
| each type a seam's signatures carry | a helper `pub` only to cross a crate |
| a name an SDK user calls, such as `Sandbox` | a re-export, registered where it is defined |

A name only kobako's own crates call is promised to no one, and its change already fails their build. A seam's change breaks a third party's implementation that kobako never compiles, which only the contract and its grade in [`guides/customization.md`](../guides/customization.md) bring to light.

## Naming Principles

A name serves two principles, and the first overrides the second.

| Principle | Means |
|---|---|
| Language first | keep what the language or its ecosystem fixes, over the rules below |
| Self-describing | receiver and name read as a phrase no comment repeats |

Because names carry the intent, a comment is left for what no name can say: a reason, a constraint, or a reference to something outside.

## Self-Contained Wording

A name, a comment, a task description, or a message says what it is about where the reader stands. A reader handed a step number or a label must leave to look it up, and loses the thread.

| Write | Not |
|---|---|
| the thing a step handles | its number in a process: the mruby archive, not Stage B |
| the outside thing by its own name | a label only one document defines: `d_make_root`, not "step 3" |
| what a check guards | the finding or round that raised it |
| an earlier item of the same list | its number: the mrbgem allowlist, not "rule #1" |

A reference to something outside stays, as long as it names that thing: a Linux function, a CRuby method, a POSIX error.

## Naming Precedence

A name is judged inside its context, as Contexts in [`architecture.md`](../architecture.md) draws them. Across two contexts only the interface that translates between them is compared, so the same word may stand in both.

A name is settled by three steps, in order; the first that decides it ends the search.

1. Keep a name the language or another side already fixes, as Fixed Names lists.
2. Give it the word class of what it names, as Word Classes and Method Names lay out.
3. Read it where it is used, as Call-Site Reading shows.

A name another side fixes may look like an exception to its word class. It stays, because a reader of that language expects it.

## Word Classes

What a name stands for decides its word class.

| What is named | Its name | As in |
|---|---|---|
| A module, class, struct, or enum | a noun, or an adjective and a noun | `Sandbox`, `FrozenWallClock` |
| A method | a verb, then its object | `mount`, `register_symbol` |
| An attribute | a noun, or the state it records | `memory_limit`, `failed?` |
| An enum variant | a noun or an adjective | `Profile::Hermetic`, `Grant::Mount` |
| A variable or field | a noun, or the state it records | `owner`, `live` |
| A callback, a method or the field holding one | `on_` and the event | `on_dispatch` |

A method names no subject, because its receiver is one: `sandbox.mount(path, fs)` reads as noun, verb, and noun. An attribute answers what a value holds and takes nothing.

## Method Names

A method is named for what it does, and each kind shows it this way.

| A method that | Its name | As in |
|---|---|---|
| acts | a verb, then its object | `preload`, `register_source` |
| finds a thing by a key | the noun, `_by_`, and the key | `error_by_name` |
| answers a part of its argument, changing nothing | the noun, then `_of` | `size_of`, `TypeId::of`, `request_of` |
| makes or converts a value | `new`, `with_`, `for_`, `from_`, `into_`, `to_` | `with_nr_inodes`, `from_posix_name` |
| adds one more of a member it already holds | the member's noun, as `Command::arg` | `flag` |
| runs a block under a condition | a preposition and the condition | `within_value_budget { … }` |

A preposition opens a name only where the call reads as a phrase without it, as `with_X(…)` and `within_X { … }` do. Elsewhere it stands after a verb.

A noun names only a method that changes nothing, as the standard library's `size_of` does. One that charges, writes, or changes its receiver acts, so it takes a verb.

### Call-Site Reading

A name reads at the call site as a phrase, without a comment repeating it.

```ruby
sandbox.mount("/data", fs)                    # subject, verb, object
Codec.within_value_budget(budget) { run }     # a preposition, then the condition

memory.emptied                                # drift: a participle names no act
memory.to_empty                               # a conversion names what it makes
```

## Participles

A word ending in `-ed` or `-ing` says what state a thing is in. It never names an action or a type alone, so it stands only here.

| Where | As in |
|---|---|
| An attribute or variable recording a state | `stdout_truncated?`, `spent` |
| An enum variant naming a state | a variant such as `Sealed` |
| After a verb | `add_hashed` |
| A dictionary noun, in that sense | `Blocking`, `nesting` |
| Before a noun, as its adjective | `FrozenWallClock`, `uncovered_lines` |
| A name another side fixes | `method_missing`, `respond_to_missing?` |

A Rust attribute recording a state keeps the same name as its Ruby peer without the `?`, so `Execution::stdout_truncated` mirrors `Execution#stdout_truncated?`.

## Questions

A question answers true or false, and each language marks it its own way.

| Language | Its name | As in |
|---|---|---|
| Ruby | ends in `?` | `failed?`, `reserved?` |
| Rust | `is_` or `has_` | `is_mounted`, `is_at_end` |
| Rust | a verb in the third person | `allows`, `matches_exactly` |
| Rust | a recorded state | `stdout_truncated`, `abandoned` |

The third-person form follows the standard library's `contains` and `starts_with`.

## Raising Methods

A Ruby method ends in `!` only as the dangerous member of a pair, as Ruby marks one. A method with no safe counterpart raises without it, as `Integer()` and `Hash#fetch` do.

| Method | Its name | As in |
|---|---|---|
| an act that raises where it could not do its work | its verb | `build_runtime`, `run_baker` |
| a guard that raises when its condition fails | the check | `ensure_capacity`, `reject_unwrappable` |
| a change to its receiver | its verb | `invalidate`, `record_handle` |
| the dangerous member of a pair | the safe member's name and `!` | `sort!`, ActiveRecord's `save!` |

Raising alone does not earn `!`, since every method may raise.

## Kinds and Markers

Two kinds of name take an adjective alone, as most languages and libraries do.

| Name | Takes | As in |
|---|---|---|
| a marker value | the state it stands for | `Kobako::Unresolved` |
| a type or constructor standing for a variant | the variant's adjective | `Unsupported`, `CodecError::unrepresentable` |

A constructor of this kind follows the standard library's `io::Error::other`.

## Limits

Four words name a bound, each for one origin.

| Word | Names | As in |
|---|---|---|
| `X_limit`, `limit` | a bound the Host App sets; `nil` or `None` turns it off | `memory_limit`, `stdout_limit` |
| `budget` | a bound worked out from another, spent as one piece of work runs | `value_budget` |
| `MAX_X` | a constant kobako fixes | `MAX_NESTING_DEPTH` |
| `X_MAX` | a constant an outside standard names, as it spells it | `PATH_MAX` |

## Files and Tests

Files and tests are named for what they hold.

| What | Its name | As in |
|---|---|---|
| A Ruby file | the constant it defines | `value_budget.rb` → `ValueBudget` |
| A Rust module | what it makes or does | `descriptors.rs`, `namei.rs` |
| A fixture | a noun, its situation in a comment | `minimal_abi_ok.wat` |
| A Rust test | the behaviour as a sentence | `the_guest_wall_clock_reads_the_unix_epoch` |
| A Minitest test | `test_`, then the behaviour | `test_a_block_run_on_another_thread_refuses` |

Test code follows every rule here, save that a test double's type is named for the behaviour it stands in for, as `CountsLookups`.

Where a file goes follows the part that owns its concern. Part Ownership in [`architecture.md`](../architecture.md) places the crates, and the comment atop `lib/kobako.rb` places the Ruby tiers. A test's assertion message follows How We Work in `AGENTS.md`.

## Fixed Names

These names come from another side, and each stays as that side spells it. The side is the layer a name stands at. The standard library's file functions wrap system calls, a layer above the VFS, so the VFS spells its names as Linux does. A type drops the prefix C needs for want of one, so `d_revalidate` is `DentryOperations::revalidate`. A function with no type to hang from keeps its whole name.

| Side | As in |
|---|---|
| Rust API Guidelines | `new`, `try_`, `as_`, `into_`, `_mut`, a getter without `get_`, a trait such as `Blocking` |
| Rust API Guidelines, where a type name drops what its module path says | `io::Error`, `kobako_wasmtime::Driver` |
| Linux, for the VFS, a Filesystem's operations, locks, and scheduling | `filename_mkdirat`, `vfs_mkdir`, `DentryOperations::revalidate`, `RwSemaphore::down_read`, `sched::schedule` |
| Linux, for `net`'s sockets | `ProtoOps`, `InetSock`, `sock_alloc_file`, `put_port`, `MAX_TCP_KEEPIDLE` |
| Ruby and its hooks | `method_missing`, `respond_to_missing?`, `LocalJumpError`, `ThreadError` |
| magnus and beni | `funcall`, `define_method` |
| WASI and POSIX | `fd_readdir`, `PATH_MAX`, `ELOOP` |
| wasmtime-wasi's preview1, for the Wasm Driver's descriptor table, where a name there means the same | `Descriptors`, `push`, `remove`, `preopen_path` |
| rustc_lexer, for a lexer's methods, each named for the token it lexes | `group`, `brace`, `octal` |
| fancy-regex, for a value kobako hands it | `BACKTRACK_LIMIT`, for `backtrack_limit` |
| MessagePack | `ext`, `bin`, `str` |
| Minitest | `test_`, `assert_equal` |
| connection_pool | `Pool#with` |
