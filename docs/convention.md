# Conventions

How code in kobako is named, beside [`architecture.md`](architecture.md), which says where it goes. Every rule applies to Rust, Ruby, and RBS alike.

| Question | Section |
|---|---|
| what a name must achieve | Principles |
| how a name is settled | Precedence |
| which word class names what | Word Classes, Method Names |
| where `-ed` and `-ing` stand | Participles |
| how a question, a raise, or a limit reads | Questions, Raising Methods, Limits |
| which names come from elsewhere | Fixed Names |

## Principles

A name serves two principles, and the first overrides the second.

| Principle | Means |
|---|---|
| Language first | keep what the language or its ecosystem fixes, over the rules below |
| Self-describing | receiver and name read as a phrase no comment repeats |

Because names carry the intent, a comment is left for what no name can say: a reason, a constraint, or a reference to something outside.

## Precedence

A name is judged inside its context, as The Contexts in [`architecture.md`](architecture.md) draws them. Across two contexts only the interface that translates between them is compared, so the same word may stand in both.

A name is settled by three steps, in order; the first that decides it ends the search.

1. Keep a name the language or another side already fixes, as Fixed Names lists.
2. Give it the word class of what it names, as Word Classes and Method Names lay out.
3. Read it where it is used, as Call-Site Reading shows.

A name kept in step 1 may look like an exception to step 2. It stays, because a reader of that language expects it.

## Word Classes

What a name stands for decides its word class.

| What is named | Its name | As in |
|---|---|---|
| A module, class, struct, or enum | a noun, or an adjective and a noun | `Sandbox`, `OpenDirectory` |
| A method | a verb, then its object | `mount`, `write_event` |
| An attribute | a noun, or the state it records | `memory_limit`, `failed?` |
| An enum variant | a noun or an adjective | `Profile::Hermetic`, `Grant::Mount` |
| A variable or field | a noun, or the state it records | `owner`, `live` |

A method names no subject, because its receiver is one: `sandbox.mount(path, fs)` reads as noun, verb, and noun. An attribute answers what a value holds and takes nothing.

## Method Names

A method is named for what it does, and each kind shows it this way.

| A method that | Its name | As in |
|---|---|---|
| acts | a verb, then its object | `preload`, `register_source` |
| finds a thing by a key | the noun, `_by_`, and the key | `error_by_name` |
| makes or converts a value | `new`, `with_`, `for_`, `from_`, `into_`, `to_` | `with_inodes`, `from_posix_name` |
| adds one more of a member it already holds | the member's noun, as `Command::arg` | `layer` |
| runs a block under a condition | a preposition and the condition | `within_value_budget { … }` |
| is called when something happens | `on_` and the event | `on_dispatch` |

A preposition opens a name only where the call reads as a phrase without it, as `with_X(…)` and `within_X { … }` do. Elsewhere it stands after a verb.

### Call-Site Reading

A name reads at the call site as a phrase, without a comment repeating it.

```ruby
sandbox.mount("/data", fs)        # subject, verb, object
decoder.decode_within(budget)     # a verb, then where it acts

memory.emptied                    # drift: a participle names no act
memory.to_empty                   # a conversion names what it makes
```

## Participles

A word ending in `-ed` or `-ing` says what state a thing is in. It never names an action or a type alone, so it stands only here.

| Where | As in |
|---|---|
| An attribute or variable recording a state | `stdout_truncated?`, `spent` |
| An enum variant naming a state | a variant such as `Closed` |
| After a verb | `write_aligned` |
| A dictionary noun, in that sense | `Blocking`, `nesting` |
| A name step 1 keeps | `method_missing`, `respond_to_missing?` |

A Rust attribute recording a state keeps the same name as its Ruby peer without the `?`, so `Execution::stdout_truncated` mirrors `Execution#stdout_truncated?`.

## Questions

A question answers true or false, and each language marks it its own way.

| Language | Its name | As in |
|---|---|---|
| Ruby | ends in `?` | `failed?`, `malformed?` |
| Rust | `is_` or `has_` | `is_mounted`, `is_at_end` |
| Rust | a verb in the third person | `allows`, `follows_links` |
| Rust | a recorded state | `stdout_truncated`, `abandoned` |

The third-person form follows the standard library's `contains` and `starts_with`.

## Raising Methods

A Ruby method ends in `!` when it raises where it could not do its work. A guard that only checks is named for the check.

| Method | Raises | As in |
|---|---|---|
| a guard | when its condition fails | `ensure_capacity!`, `reject_unwrappable!` |
| a pair | the `!` member, where the other answers | `save` and `save!` |
| a change | never, so no `!` | `invalidate`, `register_source` |

A change that only updates its receiver is told by its verb, not by `!`.

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
| `X_MAX` | a constant an outside standard names, kept by step 1 | `PATH_MAX` |

## Files and Tests

Files and tests are named for what they hold.

| What | Its name | As in |
|---|---|---|
| A Ruby file | the constant it defines | `value_budget.rb` → `ValueBudget` |
| A Rust module | what it makes or does | `descriptors.rs`, `namei.rs` |
| A fixture | a noun, its situation in a comment | `minimal_abi_ok.wat` |
| A Rust test | the behaviour as a sentence | `the_guest_wall_clock_reads_the_unix_epoch` |
| A Minitest test | `test_`, then the behaviour | `test_a_block_run_on_another_thread_refuses` |

Where a file goes follows How We Work in `AGENTS.md`, and so does a test's assertion message.

## Fixed Names

These names come from another side, and step 1 keeps each as that side spells it. The side is the layer a name stands at. The standard library's file functions wrap system calls, a layer above the VFS, so the VFS spells its names as Linux does. A type drops the prefix C needs for want of one, so `d_revalidate` is `DentryOperations::revalidate`. A function with no type to hang from keeps its whole name.

| Side | As in |
|---|---|
| Rust API Guidelines | `new`, `try_`, `as_`, `into_`, `_mut`, a getter without `get_`, a trait such as `Blocking` |
| Rust standard library | `OpenOptions`, `Metadata`, `DirEntry` |
| Linux, for the VFS and a Filesystem's operations | `filename_mkdirat`, `vfs_mkdir`, `DentryOperations::revalidate` |
| Ruby and its hooks | `method_missing`, `respond_to_missing?`, `LocalJumpError`, `ThreadError` |
| magnus and beni | `funcall`, `define_method` |
| WASI and POSIX | `fd_readdir`, `PATH_MAX`, `ELOOP` |
| MessagePack | `ext`, `bin`, `str` |
| Minitest | `test_`, `assert_equal` |
| connection_pool | `Pool#with` |
