# Payload wire

What the payload codec will carry between host and guest, and what it refuses rather than change on the way.

## Includes

- `test/unit/codec/test_handle_walk_nesting.rb`
- `test/unit/codec/test_unrepresentable_guard.rb`
- `test/unit/codec/test_track_handles.rb`
- `test/e2e/test_byte_fidelity.rb`
- `test/e2e/test_integer_range.rb`
- `test/e2e/test_answer_value_refusal.rb`
- `test/fuzz/test_roundtrip_fuzz.rb`
- `test/fuzz/test_guest_value_fuzz.rb`
- `wasm/kobako-mruby/src/refusal.rs`

### Why these scenarios

A codec that changes a value on the way is worse than one that refuses it, because the caller reads a plausible answer and never learns it was not the one sent. So every refusal here is paired against the value just inside the bound it refuses: an integer at the guest's widest, nesting at the deepest the wire encodes, a keyword name that is text after all.

The bounds are reached from three directions — an answer, an argument, a yield — and each is witnessed, since a check placed on one path leaves the others carrying whatever they were given.

Two implementations of this codec exist on the host and a third inside the guest. The first two are held to each other byte for byte; the third has no peer, so it is held to an identity law instead. Both are properties over generated values rather than statements about one, which is why each is a single scenario.

Whether a decode carried a capability reference only decides whether a later walk is worth taking, so a wrong answer costs time rather than correctness. It is declared anyway, in both directions and across two brackets, because the walk it skips is the one that resolves references — and a signal stuck at either answer stops being a signal quietly.

What the codec does with a value it accepts — which of the eleven type mappings each shape takes, how a length is framed, what a malformed frame answers — is the encoding table rather than the boundary. It is specified with the wire format, and its scenarios are the payload encoding feature's.

## `CD-001` A Service answer past the guest's integer width is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service answering an integer wider than the guest carries |
| When | guest code calls it |
| Then | the invocation fails naming the guest's integer range |

## `CD-002` The widest integer the guest carries still crosses

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service answering the largest integer the guest carries |
| When | guest code calls it |
| Then | the answer comes back unchanged |

## `CD-003` A run argument past the guest's integer width fails at entry

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a preloaded entrypoint |
| When | `#run` carries an integer wider than the guest carries |
| Then | the invocation fails before the entrypoint is reached |

## `CD-004` A yield argument past the guest's integer width is refused at the yield

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service yielding an integer wider than the guest carries |
| When | guest code calls it with a block |
| Then | the failure reaches the Host App as the Service's |

## `CD-005` A string the wire cannot read as text keeps its bytes

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation answers a String whose bytes are not text |
| Then | the host receives those bytes as binary |

## `CD-006` A name that is not text is refused rather than carried as bytes

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | an evaluation answers a Symbol whose bytes are not text |
| Then | the invocation fails naming the unsupported type |

## `CD-007` A dispatch argument's bytes reach the Service intact

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service recording what it receives |
| When | guest code passes a String whose bytes are not text |
| Then | the Service received those bytes |

## `CD-008` A dispatch argument that is a name and not text is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service |
| When | guest code passes a Symbol whose bytes are not text |
| Then | the guest sees a `TypeError` naming the argument's type |

## `CD-009` A keyword name that is not text is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a bound Service |
| When | guest code passes a keyword whose name's bytes are not text |
| Then | the guest sees a `TypeError` |

## `CD-010` A keyword name written as text is a keyword name

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service recording the keywords it receives |
| When | guest code passes a keyword whose name is written as a String |
| Then | the Service received it under that name |

## `CD-011` An argument at the deepest nesting the wire encodes crosses unchanged

| Step | Statement |
| --- | --- |
| Given | a value nested to the deepest level the wire encodes |
| When | the wrap walk carries it |
| Then | it comes through unchanged |

## `CD-012` One level deeper is refused

| Step | Statement |
| --- | --- |
| Given | a value nested one level past what the wire encodes |
| When | the wrap walk carries it |
| Then | `Kobako::SandboxError` names the depth bound |

## `CD-013` A value that refers to itself is refused rather than followed

| Step | Statement |
| --- | --- |
| Given | a value holding a reference to itself |
| When | the wrap walk carries it |
| Then | `Kobako::SandboxError` is raised |

## `CD-014` So is one standing as a key

| Step | Statement |
| --- | --- |
| Given | a Hash whose key holds a reference to itself |
| When | the wrap walk carries it |
| Then | `Kobako::SandboxError` is raised |

## `CD-015` A value outside the type mapping is refused, not probed

| Step | Statement |
| --- | --- |
| Given | an object whose missing-method handler answers any call |
| When | the encoder is asked to write it |
| Then | it refuses the type rather than writing what the probe answered |

## `CD-016` An object with no class surface at all is refused the same way

| Step | Statement |
| --- | --- |
| Given | an object built without the ordinary class surface |
| When | the encoder is asked to write it |
| Then | it refuses the type |

## `CD-017` The guard's own marker is not a value the wire carries

| Step | Statement |
| --- | --- |
| Given | bytes carrying the guard's extension id |
| When | the decoder reads them |
| Then | it refuses the type |

## `CD-018` An answer that nests without end is the Service's failure

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service answering a value that nests without end |
| When | guest code calls it and leaves the failure unrescued |
| Then | `Kobako::ServiceError` reaches the Host App |

## `CD-019` That refusal is worded as kobako's own

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose Service answered a value that nests without end |
| When | the Host App reads the failure's message |
| Then | it does not wear the shape a Service's own exception crosses in |

## `CD-020` The guest may rescue it and carry on

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Service answering a value that nests without end |
| When | guest code rescues the failure and returns a value |
| Then | the invocation answers that value |

## `CD-021` The two host implementations write the same bytes

| Step | Statement |
| --- | --- |
| Given | generated values covering the shapes the wire carries |
| When | each is written by both host implementations and read back by each |
| Then | the bytes match and every value comes back as itself |

## `CD-022` The guest's own walk returns what it was given

| Step | Statement |
| --- | --- |
| Given | generated values covering the shapes the wire carries |
| When | each is sent to the guest and answered back |
| Then | it comes back as itself |

## `CD-023` A decode says whether the value it read carried a reference

| Step | Statement |
| --- | --- |
| Given | a payload whose tree carries a capability reference |
| When | it is decoded inside a tracking bracket |
| Then | the bracket reports a reference was carried, and answers the decoded value unchanged |

## `CD-024` And says so when it carried none

| Step | Statement |
| --- | --- |
| Given | a payload whose tree carries no capability reference |
| When | it is decoded inside a tracking bracket |
| Then | the bracket reports none was carried, and answers the decoded value unchanged |

## `CD-025` One bracket's sighting is not the next one's

| Step | Statement |
| --- | --- |
| Given | a bracket on this thread that decoded a payload carrying a reference |
| When | a second bracket on the same thread decodes a payload carrying none |
| Then | the second reports none was carried |

## `CD-026` A value the schema cannot write is the script's own type error

| Step | Statement |
| --- | --- |
| Given | a value the schema cannot write, handed over at a position a script reached |
| When | the refusal is raised |
| Then | it is the class a script sees for handing over the wrong type |

## `CD-027` A value the schema cannot write at the invocation's own position fails the invocation

| Step | Statement |
| --- | --- |
| Given | a value the schema cannot write, standing as the invocation's own result |
| When | the refusal is made |
| Then | the invocation fails, no guest frame being left to raise into |

## `CD-028` That refusal names the type the codec reported

| Step | Statement |
| --- | --- |
| Given | a value the schema cannot write, standing as the invocation's own result |
| When | the refusal's message is read |
| Then | it names the type rather than describing the value |

## `CD-029` The interpreter's own limit travels as a wire failure everywhere

| Step | Statement |
| --- | --- |
| Given | a value the interpreter itself cannot carry, at each position there is |
| When | the refusal is made |
| Then | each is an exchange that did not complete, not a schema or script fault |

## `CD-030` A message that could not be carried names its position and direction

| Step | Statement |
| --- | --- |
| Given | a message the codec could not carry, at each position there is |
| When | the refusal's message is read |
| Then | it says what was being carried and which way it was going |

## `CD-031` A run payload words each refusal distinctly

| Step | Statement |
| --- | --- |
| Given | each kind of refusal a run's arguments can meet |
| When | their messages are read |
| Then | no two are worded alike, that message being the only account of why nothing ran |

## `CD-032` A position the schema does not serve refuses as unimplemented

| Step | Statement |
| --- | --- |
| Given | a position this schema does not serve, reached from a guest frame |
| When | the refusal is raised |
| Then | it is the class a bare rescue does not swallow |

## `CD-033` A schema serving a Call but not its Reply leaves the exchange half-served

| Step | Statement |
| --- | --- |
| Given | a schema that writes a Call and cannot read its Reply |
| When | the refusal is raised |
| Then | it is an exchange that did not complete, not a missing capability |

## `CD-034` A position only the host reads fails the invocation when the capability is absent

| Step | Statement |
| --- | --- |
| Given | a position only the host reads, whose capability this schema does not serve |
| When | the refusal is made |
| Then | the invocation fails, no guest frame being left to raise into |

## `CD-035` A refusal raised into a guest frame names a class that frame can raise

| Step | Statement |
| --- | --- |
| Given | each refusal reachable at the two positions delivered into a live guest frame |
| When | the class each names is read |
| Then | each is a class the guest can resolve |
