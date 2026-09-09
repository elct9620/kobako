# Core envelope

The bytes that say where a message goes and how it turned out, and the fixed values both sides must already agree on to read them.

## Includes

- `crates/kobako-transport/src/**/*.rs`
- `crates/kobako-wasmtime/src/guest_mem.rs`
- `wasm/kobako-core/src/frames.rs`

### Why these scenarios

This is the one tier with a single implementation: the envelope and the ABI's values are defined once and both sides read that definition, so nothing here is witnessed by a second implementation the way the payload wire is. What answers for it instead is the byte layout itself — a fixed-layout field order pinned to specific bytes, so a change that would leave one peer reading a different message cannot pass as a refactor.

The envelope is readable without a Codec, and the scenarios are written to keep it that way. A payload crosses as bytes the envelope never parses, which is why one that is not a value at all still arrives intact; routing and attribution are read from the envelope's own fields alone.

Every field is refused rather than repaired. A length that overruns the message, a count larger than what follows it, a tag outside the set, a flag that is neither of its two values — each is a wire violation, because a reader that repaired one would leave the two peers disagreeing about what was sent. The refusals are declared beside the round-trips because a shape that only round-trips says nothing about what a reader does with bytes this writer would never emit.

Two forward-compatibility rules are declared as behavior rather than left to a version bump: a Fault kind this reader predates still delivers its message, and a field it predates is skipped. They are what lets the reserved and unknown tags elsewhere be refusals — the extension point is named, so everything outside it can be closed.

## `WE-001` The packed answer carries the pointer high and the length low

| Step | Statement |
| --- | --- |
| Given | a pointer and a length |
| When | they are packed into the answer the ABI's buffer-returning functions give |
| Then | the pointer occupies the high half and the length the low half |

## `WE-002` Every pointer and length pair survives the packing

| Step | Statement |
| --- | --- |
| Given | each bound of the pointer and length ranges |
| When | the pair is packed and read back |
| Then | it arrives as it was given |

## `WE-003` Every field the envelope is built from round-trips

| Step | Statement |
| --- | --- |
| Given | each kind of field the envelope is written from |
| When | they are written and read back |
| Then | each arrives as it was given |

## `WE-004` A number crosses big-endian

| Step | Statement |
| --- | --- |
| Given | a number written to the envelope |
| When | the bytes are read |
| Then | the most significant byte comes first |

## `WE-005` A length running past the message is refused

| Step | Statement |
| --- | --- |
| Given | a field declaring more bytes than the message carries |
| When | it is read |
| Then | it is refused rather than truncated |

## `WE-006` A count the message cannot satisfy is refused

| Step | Statement |
| --- | --- |
| Given | a list declaring more elements than the message could carry |
| When | it is read |
| Then | it is refused before anything is allocated for it |

## `WE-007` Text that is not text is refused

| Step | Statement |
| --- | --- |
| Given | a text field carrying bytes that are not text |
| When | it is read |
| Then | it is refused |

## `WE-008` Bytes past the last field are refused

| Step | Statement |
| --- | --- |
| Given | a message carrying bytes past its last field |
| When | it is read to the end |
| Then | it is refused as a framing desync |

## `WE-009` A Call naming a constant path round-trips

| Step | Statement |
| --- | --- |
| Given | a Call whose target is a constant path |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WE-010` A Call naming a capability reference round-trips

| Step | Statement |
| --- | --- |
| Given | a Call whose target is a capability reference |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WE-011` A payload the envelope cannot read still crosses

| Step | Statement |
| --- | --- |
| Given | a Call whose payload is not a value any Codec would write |
| When | it is written and read back |
| Then | the payload arrives unchanged |

## `WE-012` A constant-path Call's byte layout is fixed

| Step | Statement |
| --- | --- |
| Given | a Call whose target is a constant path |
| When | it is written |
| Then | the bytes are the path's kind, then target, method, block flag, and payload in that order |

## `WE-013` A Call naming a capability reference carries the bare id

| Step | Statement |
| --- | --- |
| Given | a Call whose target is a capability reference |
| When | it is written |
| Then | the id follows the kind byte on its own, with no length before it |

## `WE-014` A target kind that is neither is refused

| Step | Statement |
| --- | --- |
| Given | a Call whose target kind is neither a path nor a capability reference |
| When | it is read |
| Then | it is refused |

## `WE-015` The invalid reference id is refused in the target position

| Step | Statement |
| --- | --- |
| Given | a Call targeting the reference id that stands for none |
| When | it is read |
| Then | it is refused |

## `WE-016` A block flag that is neither of its values is refused

| Step | Statement |
| --- | --- |
| Given | a Call whose block flag is neither set nor clear |
| When | it is read |
| Then | it is refused rather than read as either |

## `WE-017` A Call whose target overruns the message is refused

| Step | Statement |
| --- | --- |
| Given | a Call whose target length runs past the message |
| When | it is read |
| Then | it is refused rather than truncated |

## `WE-018` Both Reply arms round-trip

| Step | Statement |
| --- | --- |
| Given | a Reply on each of its two arms |
| When | each is written and read back |
| Then | each arrives as it was given |

## `WE-019` An empty answer is an answer

| Step | Statement |
| --- | --- |
| Given | a Reply whose body carries nothing |
| When | it is read |
| Then | it arrives as an empty body rather than a truncation |

## `WE-020` A Reply tag outside the pair is refused

| Step | Statement |
| --- | --- |
| Given | a Reply tagged as neither of its two arms |
| When | it is read |
| Then | it is refused |

## `WE-021` A Reply carrying nothing at all is refused

| Step | Statement |
| --- | --- |
| Given | a Reply of no length |
| When | it is read |
| Then | it is refused rather than read as a missing tag |

## `WE-022` Every live arm of a yield's Reply round-trips

| Step | Statement |
| --- | --- |
| Given | a yield's Reply on each arm the wire still serves |
| When | each is written and read back |
| Then | each arrives as it was given |

## `WE-023` The reserved yield tag is refused

| Step | Statement |
| --- | --- |
| Given | a yield's Reply carrying the tag held in reserve |
| When | it is read |
| Then | it is refused |

## `WE-024` A yield tag outside the live set is refused

| Step | Statement |
| --- | --- |
| Given | a yield's Reply tagged outside the arms the wire serves |
| When | it is read |
| Then | it is refused |

## `WE-025` Each Reply arm's tag byte is fixed

| Step | Statement |
| --- | --- |
| Given | a Reply on each of its two arms |
| When | each is written |
| Then | each carries its own tag byte followed by that arm's body alone |

## `WE-026` Each yield arm's tag byte is fixed

| Step | Statement |
| --- | --- |
| Given | a yield's Reply on each live arm |
| When | each is written |
| Then | each carries the tag byte that arm is assigned |

## `WE-027` An Outcome carrying a value round-trips

| Step | Statement |
| --- | --- |
| Given | an Outcome on its succeeding arm |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WE-028` A Panic round-trips

| Step | Statement |
| --- | --- |
| Given | an Outcome carrying a Panic |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WE-029` A Panic offering a correction carries its names in order

| Step | Statement |
| --- | --- |
| Given | a Panic offering the names the caller could have meant |
| When | it is written and read back |
| Then | the names arrive in the order they were given |

## `WE-030` A Panic offering no correction decodes as an empty list

| Step | Statement |
| --- | --- |
| Given | a Panic with no name to offer |
| When | it is read |
| Then | the list is present and empty rather than a refusal |

## `WE-031` Attribution is read from the origin alone

| Step | Statement |
| --- | --- |
| Given | a Panic written with a side of the boundary named as its origin |
| When | it is read |
| Then | it attributes to that side |

## `WE-032` Bytes past a Panic's last field are refused

| Step | Statement |
| --- | --- |
| Given | a Panic carrying bytes past the names it offers |
| When | it is read |
| Then | it is refused as a framing desync |

## `WE-033` An origin outside the reserved set attributes to the Sandbox

| Step | Statement |
| --- | --- |
| Given | a Panic whose origin is outside the set this reader knows |
| When | it is read |
| Then | it attributes to the Sandbox |

## `WE-034` The succeeding arm's tag byte is fixed

| Step | Statement |
| --- | --- |
| Given | an Outcome on its succeeding arm |
| When | it is written |
| Then | the tag byte is followed by the value alone |

## `WE-035` A Panic's field order is fixed

| Step | Statement |
| --- | --- |
| Given | an Outcome carrying a Panic |
| When | it is written |
| Then | the origin stands before the Error Record |

## `WE-036` An outcome buffer carrying nothing is refused

| Step | Statement |
| --- | --- |
| Given | an outcome buffer of no length |
| When | it is read |
| Then | it is refused |

## `WE-037` An Outcome tag outside the pair is refused

| Step | Statement |
| --- | --- |
| Given | an Outcome tagged as neither of its two arms |
| When | it is read |
| Then | it is refused |

## `WE-038` Every Fault kind round-trips

| Step | Statement |
| --- | --- |
| Given | a Fault of each kind |
| When | each is written and read back |
| Then | each arrives as it was given |

## `WE-039` A Fault kind this reader predates still delivers its message

| Step | Statement |
| --- | --- |
| Given | a Fault of a kind added after this reader was built |
| When | it is read |
| Then | the message arrives under the undefined kind |

## `WE-040` A Fault field this reader predates is skipped

| Step | Statement |
| --- | --- |
| Given | a Fault carrying a field added after this reader was built |
| When | it is read |
| Then | the fields this reader knows arrive unchanged |

## `WE-041` The Fault byte layout is kind then message

| Step | Statement |
| --- | --- |
| Given | a Fault |
| When | it is written |
| Then | the kind stands before the message |

## `WE-042` A Fault kind's name reads back as the same kind

| Step | Statement |
| --- | --- |
| Given | the name of each Fault kind |
| When | each name is read |
| Then | each resolves to the kind it names |

## `WE-043` A Fault name this build predates resolves to no kind

| Step | Statement |
| --- | --- |
| Given | a Fault name this build does not know |
| When | it is read |
| Then | it resolves to no kind |

## `WE-044` A Run round-trips

| Step | Statement |
| --- | --- |
| Given | a Run naming an entrypoint and carrying its arguments |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WE-045` A Run carrying no arguments round-trips

| Step | Statement |
| --- | --- |
| Given | a Run whose payload carries nothing |
| When | it is read |
| Then | it arrives empty rather than as a truncation |

## `WE-046` The Run byte layout is fixed

| Step | Statement |
| --- | --- |
| Given | a Run |
| When | it is written |
| Then | its fields stand in the order both peers read them in |

## `WE-047` An Error Record round-trips

| Step | Statement |
| --- | --- |
| Given | an Error Record |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WE-048` A failure with no backtrace round-trips

| Step | Statement |
| --- | --- |
| Given | an Error Record whose backtrace carries nothing |
| When | it is read |
| Then | the backtrace is present and empty rather than a refusal |

## `WE-049` The Error Record byte layout is name, message, then backtrace

| Step | Statement |
| --- | --- |
| Given | an Error Record |
| When | it is written |
| Then | the name stands before the message, and the message before the backtrace |

## `WE-050` A Sandbox with no bindings still sends the Frame

| Step | Statement |
| --- | --- |
| Given | a Sandbox that has bound nothing |
| When | its bindings Frame is written and read back |
| Then | the Frame is present and empty |

## `WE-051` Every binding path round-trips

| Step | Statement |
| --- | --- |
| Given | a binding at each shape of path a Sandbox accepts |
| When | the Frame is written and read back |
| Then | each path arrives as it was given |

## `WE-052` Both preload kinds round-trip in order

| Step | Statement |
| --- | --- |
| Given | preloads of both kinds |
| When | the Frame is written and read back |
| Then | they arrive in the order they were given |

## `WE-053` A Sandbox with no preloads still sends the Frame

| Step | Statement |
| --- | --- |
| Given | a Sandbox that has preloaded nothing |
| When | its preload Frame is written and read back |
| Then | the Frame is present and counts nothing |

## `WE-054` The bindings Frame is a counted list

| Step | Statement |
| --- | --- |
| Given | a Sandbox carrying bindings |
| When | the Frame is written |
| Then | a count stands before the paths it counts |

## `WE-055` A preload entry's shape is fixed

| Step | Statement |
| --- | --- |
| Given | a preload |
| When | the Frame is written |
| Then | the entry's fields stand in the order both peers read them in |

## `WE-056` A preload kind that is neither is refused

| Step | Statement |
| --- | --- |
| Given | a preload whose kind is neither of the two the Frame serves |
| When | the Frame is read |
| Then | it is refused |

## `WE-057` A preload count the Frame cannot satisfy is refused

| Step | Statement |
| --- | --- |
| Given | a Frame counting more preloads than it carries |
| When | it is read |
| Then | it is refused before anything is allocated for it |

## `WE-058` Bytes past a Frame's last field are refused

| Step | Statement |
| --- | --- |
| Given | a Frame carrying bytes past its last field |
| When | it is read |
| Then | it is refused as a framing desync |

## `WE-059` A transfer at the cap is admitted

| Step | Statement |
| --- | --- |
| Given | a transfer of no bytes, and one of exactly the size cap |
| When | each length is checked |
| Then | each is admitted |

## `WE-060` A transfer past the cap is refused

| Step | Statement |
| --- | --- |
| Given | a transfer one byte past the size cap |
| When | its length is checked |
| Then | it is refused |

## `WE-061` A buffer the guest names is read half-open

| Step | Statement |
| --- | --- |
| Given | a pointer and a length inside the guest's memory |
| When | the range is taken |
| Then | it ends one past its last byte |

## `WE-062` A buffer of no length is read at any pointer inside memory

| Step | Statement |
| --- | --- |
| Given | a pointer inside the guest's memory and a length of nothing |
| When | the range is taken |
| Then | it is empty rather than refused |

## `WE-063` A pointer and length that overflow are refused

| Step | Statement |
| --- | --- |
| Given | a pointer and a length whose sum cannot be held |
| When | the range is taken |
| Then | it is refused |

## `WE-064` A buffer ending past the guest's memory is refused

| Step | Statement |
| --- | --- |
| Given | a pointer and a length ending past the guest's memory |
| When | the range is taken |
| Then | it is refused |

## `WE-065` A packed answer of nothing is the failure signal

| Step | Statement |
| --- | --- |
| Given | a packed answer of nothing |
| When | it is read back |
| Then | both halves are nothing |

## `WE-066` A length-prefixed frame round-trips

| Step | Statement |
| --- | --- |
| Given | a payload written behind its length |
| When | the frame is read |
| Then | the payload arrives as it was given |

## `WE-067` A frame declaring more than the ceiling is refused

| Step | Statement |
| --- | --- |
| Given | a frame whose length prefix declares more than the allocation ceiling |
| When | it is read |
| Then | it is refused before anything is allocated for it |
