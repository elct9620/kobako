# Payload wire

What the payload codec will carry between host and guest, and what it refuses rather than change on the way.

## Includes

- `test/unit/codec/test_handle_walk_nesting.rb`
- `test/unit/codec/test_unrepresentable_guard.rb`
- `test/e2e/test_byte_fidelity.rb`
- `test/e2e/test_integer_range.rb`
- `test/e2e/test_answer_value_refusal.rb`
- `test/fuzz/test_roundtrip_fuzz.rb`
- `test/fuzz/test_guest_value_fuzz.rb`

### Why these scenarios

A codec that changes a value on the way is worse than one that refuses it, because the caller reads a plausible answer and never learns it was not the one sent. So every refusal here is paired against the value just inside the bound it refuses: an integer at the guest's widest, nesting at the deepest the wire encodes, a keyword name that is text after all.

The bounds are reached from three directions — an answer, an argument, a yield — and each is witnessed, since a check placed on one path leaves the others carrying whatever they were given.

Two implementations of this codec exist on the host and a third inside the guest. The first two are held to each other byte for byte; the third has no peer, so it is held to an identity law instead. Both are properties over generated values rather than statements about one, which is why each is a single scenario.

What the codec does with a value it accepts — which of the eleven type mappings each shape takes, how a length is framed, what a malformed frame answers — is the encoding table rather than the boundary, and is specified with the wire format.

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
