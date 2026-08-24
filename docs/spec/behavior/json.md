# Guest JSON

What the guest's JSON surface reads, what it writes, and what it refuses to let across.

## Why these scenarios

Reading and writing JSON is compute the guest does on its own, so the scenarios follow what each direction produces and, more importantly, what each refuses. A value with no JSON form is refused rather than approximated: an integer too large to represent exactly, a number that is not finite, bytes that are not text, a key that is not a scalar.

The boundary scenarios are the reason this surface can be offered at all. Parsing yields data and never a capability, whatever the document is shaped like; generating refuses a capability reference wherever it sits — bare, nested inside what an object opted in, or standing as a key — and refuses it in the guest rather than by asking the host what to do with it.

The depth bound is witnessed on both directions at the same depth, because a reader and a writer that disagree about it would let a document in that cannot be written back out.

## JS-001 — Every JSON value reads as its native counterpart

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a document holding each JSON value kind |
| Then | each maps to its native type, with String keys |

## JS-002 — A document need not be a container

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a bare JSON number |
| Then | it yields that scalar |

## JS-003 — Object members keep the order they were written in

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses an object whose members are not in sorted order |
| Then | the resulting Hash carries them in the document's order |

## JS-004 — Malformed input is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a document that is not JSON |
| Then | a parser error is raised |

## JS-005 — The guest may carry on past a parse failure

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code rescues the parser error and returns a value |
| Then | the invocation answers that value |

## JS-006 — An integer that fits stays an integer

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses the largest value the guest's Integer width holds |
| Then | it yields an Integer |

## JS-007 — A small negative integer stays an integer too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a small negative integer |
| Then | it yields that Integer |

## JS-008 — An integer past the width widens rather than wraps

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a value one past the guest's Integer width |
| Then | it yields a Float |

## JS-009 — A widened integer is still exact where a Float can be

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses the largest integer a Float represents exactly |
| Then | it yields that value as an exact Float |

## JS-010 — An integer too large to represent is refused, not rounded

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses an integer beyond exact Float representation |
| Then | a parser error is raised |

## JS-011 — A JSON real reads as a Float

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a JSON real |
| Then | it yields a Float |

## JS-012 — Exponent form is a real like any other

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a real written in exponent form |
| Then | it yields a Float |

## JS-013 — Asking for Symbol keys gets them at every level

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a nested document asking for symbolized names |
| Then | every object at every level is keyed by Symbols |

## JS-014 — Keys are Strings unless asked otherwise

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a document without asking for symbolized names |
| Then | the keys are Strings |

## JS-015 — Asking for Strings explicitly gets Strings

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a document asking for symbolized names to be off |
| Then | the keys are Strings |

## JS-016 — An option is read by its name, not by its spelling

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a document passing the symbolize option under a String key |
| Then | the keys are Strings |

## JS-017 — Symbolizing touches keys and nothing else

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a document with String values asking for symbolized names |
| Then | the values are still Strings |

## JS-018 — Native values write as compact JSON

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from a structure of native values |
| Then | it emits compact, well-formed JSON |

## JS-019 — A control character is written as its escape

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from a String holding control characters |
| Then | each appears as its JSON escape |

## JS-020 — What was escaped reads back as what it was

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses back what it generated from a String needing escapes |
| Then | it recovers the original String |

## JS-021 — A Symbol is written as its name

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from a structure using a Symbol as key and as value |
| Then | each appears as that Symbol's name |

## JS-022 — An Integer key is written as a String

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from a Hash keyed by an Integer |
| Then | the key appears as that Integer's string form |

## JS-023 — A Float key is written as a String too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from a Hash keyed by a Float |
| Then | the key appears as that Float's string form |

## JS-024 — A key that is not a scalar has no string form to take

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from a Hash keyed by a container |
| Then | a generator error is raised |

## JS-025 — A number JSON cannot write is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from a Float that is not a finite number |
| Then | a generator error is raised |

## JS-026 — A String that is not text is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from a String whose bytes are not valid text |
| Then | a generator error is raised |

## JS-027 — A Symbol whose name is not text is refused as well

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from a Symbol whose name is not valid text |
| Then | a generator error is raised |

## JS-028 — An Array subclass is written as an array

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from an instance of an Array subclass |
| Then | it emits a JSON array |

## JS-029 — A Hash subclass is written as an object

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from an instance of a Hash subclass |
| Then | it emits a JSON object |

## JS-030 — A String subclass is written as a string

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from an instance of a String subclass |
| Then | it emits a JSON string |

## JS-031 — Pretty printing indents, and leaves empty containers alone

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code pretty-prints a structure holding empty containers |
| Then | it emits the indented layout with those containers inline |

## JS-032 — Pretty printing changes the layout and not the document

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses back what it pretty-printed |
| Then | it recovers the same tree |

## JS-033 — An object opts in by saying what it is

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from an object whose opt-in hook answers a native value |
| Then | it emits that value's JSON |

## JS-034 — What the opt-in hook answers is encoded like anything else

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from an object whose opt-in hook answers a structure |
| Then | it emits that structure's JSON |

## JS-035 — An object that never opted in is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from a plain object |
| Then | a generator error is raised |

## JS-036 — Only the opt-in hook opts an object in

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from an object that overrode the other serialization method instead |
| Then | a generator error is raised |

## JS-037 — Parsing at the depth bound succeeds

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a document nested to the bound |
| Then | it yields the structure |

## JS-038 — Parsing past the depth bound is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code parses a document nested one level past the bound |
| Then | a parser error is raised |

## JS-039 — Generating at the same depth bound succeeds

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from a structure nested to the bound |
| Then | it emits the document |

## JS-040 — Generating past the depth bound is refused at the same depth

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| When | guest code generates from a structure nested one level past the bound |
| Then | a generator error is raised |

## JS-041 — Parsing produces data, never a capability

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary with a Service bound |
| When | guest code parses a document shaped like a capability reference |
| Then | it yields an ordinary Hash |

## JS-042 — A capability reference has no JSON form

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| Given | guest code holding a capability reference |
| When | it generates from that reference |
| Then | a generator error is raised |

## JS-043 — Nesting a capability reference does not hide it

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| Given | an object whose opt-in hook answers a structure holding a capability reference |
| When | guest code generates from that object |
| Then | a generator error is raised |

## JS-044 — Using a capability reference as a key does not hide it either

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| Given | a Hash keyed by a capability reference |
| When | guest code generates from that Hash |
| Then | a generator error is raised |

## JS-045 — Asking a capability reference to serialize itself stays in the guest

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the JSON-capable Guest Binary |
| Given | guest code holding a capability reference |
| When | it calls the opt-in hook on that reference |
| Then | the raise happens in the guest without reaching the host |
