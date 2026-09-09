# Payload encoding

Which form each value takes on the payload wire, and what a frame that is not one answers.

## Includes

- `test/unit/payload/test_arguments.rb`
- `test/unit/payload/test_arguments_roundtrip.rb`
- `test/unit/codec/test_scalars.rb`
- `test/unit/codec/test_containers.rb`
- `test/unit/codec/test_ext_types.rb`
- `test/unit/codec/test_malformed.rb`
- `test/unit/codec/test_golden_vectors.rb`
- `crates/kobako-codec/src/msgpack/**/*.rs`

### Why these scenarios

The boundary this codec keeps — which values it refuses rather than change — is specified with the codec itself. What is here is the other half: the form each accepted value takes, and what a frame that is not a value answers. A caller never sees this half directly, which is exactly why it is declared: the two independent implementations agree only if both read the same table, and a table nobody wrote down is one each side is free to drift from.

Both implementations witness this one table, which is what makes a scenario here worth more than a scenario each: where the two agree, the same statement is answered twice from independently written code, and where they do not, the difference has to be declared rather than discovered. The reader's and writer's own state is declared alongside the tiers, since a tier is only ever observed through them.

Every encoding tier is witnessed at its bound and just past it, because a tier chosen one step too wide still round-trips through the writer that chose it. The byte vectors are what catch that: a value whose form is fixed to specific bytes cannot silently be promoted, and the narrowest tag for each empty container is pinned for the same reason.

Round-trips answer for what survives; the refusals answer for what a reader is handed by something that is not this writer. Those are separate observations even where one value reaches both — a reference the writer would never emit with a zero identifier still arrives with one when the bytes were built by hand.

Two asymmetries are declared as they are rather than as they should be. One writer will emit nesting its own reader refuses, because the library beneath it bounds the reader alone; nothing crosses that should not, since the writer at the other end carries the bound and refuses there. And a cyclic map is beyond reach entirely — the walk that would hit the cycle runs in frames carrying no guard, so the process ends before anything here could answer. That case is left without a scenario, since a test that cannot run witnesses nothing.

## `WP-001` An invocation's arguments cross in both positions

| Step | Statement |
| --- | --- |
| Given | positional and keyword arguments |
| When | they are written and read back |
| Then | both arrive as they were given |

## `WP-002` An invocation carrying no arguments still carries both positions

| Step | Statement |
| --- | --- |
| Given | an argument frame carrying nothing in either position |
| When | it is read |
| Then | both positions are present and empty |

## `WP-003` A keyword name that is not a name is refused

| Step | Statement |
| --- | --- |
| Given | an argument frame whose keyword key is not a name |
| When | it is read |
| Then | it is refused |

## `WP-004` A positional position that is not a list is refused

| Step | Statement |
| --- | --- |
| Given | an argument frame whose positional half is not a list |
| When | it is read |
| Then | it is refused |

## `WP-005` A frame carrying the wrong number of positions is refused

| Step | Statement |
| --- | --- |
| Given | an argument frame carrying other than two positions |
| When | it is read |
| Then | it is refused |

## `WP-006` A capability reference may ride an argument position

| Step | Statement |
| --- | --- |
| Given | an argument frame carrying a capability reference among its positional values |
| When | it is written and read back |
| Then | the reference arrives as it was given |

## `WP-007` The two implementations agree on the argument frame

| Step | Statement |
| --- | --- |
| Given | an argument frame written by this host |
| When | the peer implementation reads it and writes it back |
| Then | the bytes are identical |

## `WP-008` And agree on one carrying a wrapped leaf

| Step | Statement |
| --- | --- |
| Given | an argument frame carrying a wrapped leaf value |
| When | the peer implementation reads it and writes it back |
| Then | the bytes are identical |

## `WP-009` Nothing crosses as nothing

| Step | Statement |
| --- | --- |
| Given | the value that stands for nothing |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-010` Truth crosses as truth

| Step | Statement |
| --- | --- |
| Given | the true value |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-011` And falsity as falsity

| Step | Statement |
| --- | --- |
| Given | the false value |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-012` The single-byte integer tier crosses at both its bounds

| Step | Statement |
| --- | --- |
| Given | each bound of the positive and negative single-byte integer tiers |
| When | each is written and read back |
| Then | each arrives as it was given |

## `WP-013` Every unsigned tier crosses at both its bounds

| Step | Statement |
| --- | --- |
| Given | each bound of the one-, two-, four- and eight-byte unsigned tiers |
| When | each is written and read back |
| Then | each arrives as it was given |

## `WP-014` Every signed tier crosses at both its bounds

| Step | Statement |
| --- | --- |
| Given | each bound of the one-, two-, four- and eight-byte signed tiers |
| When | each is written and read back |
| Then | each arrives as it was given |

## `WP-015` An integer past the widest tier is refused

| Step | Statement |
| --- | --- |
| Given | an integer above the unsigned maximum, and one below the signed minimum |
| When | each is written |
| Then | each is refused as a value the mapping does not carry |

## `WP-016` A float crosses with its bits intact

| Step | Statement |
| --- | --- |
| Given | zero and its signed twin, the unit values, a fraction, the widest magnitudes and both infinities |
| When | each is written and read back |
| Then | each arrives equal, and signed zero keeps its sign |

## `WP-017` A float that is not a number arrives as one that is not

| Step | Statement |
| --- | --- |
| Given | the float that is not a number |
| When | it is written and read back |
| Then | it arrives as a float that is not a number |

## `WP-018` Text of no length crosses

| Step | Statement |
| --- | --- |
| Given | text carrying no characters |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-019` Text arrives as text

| Step | Statement |
| --- | --- |
| Given | text of single-byte characters |
| When | it is written and read back |
| Then | it arrives unchanged and as text |

## `WP-020` Text beyond one byte a character arrives as text too

| Step | Statement |
| --- | --- |
| Given | text of multibyte characters |
| When | it is written and read back |
| Then | it arrives unchanged and as text |

## `WP-021` Text crosses either side of the first length tier

| Step | Statement |
| --- | --- |
| Given | text at each bound of the first widening of the text length |
| When | each is written and read back |
| Then | each arrives as it was given |

## `WP-022` And either side of the second

| Step | Statement |
| --- | --- |
| Given | text at each bound of the second widening of the text length |
| When | each is written and read back |
| Then | each arrives as it was given |

## `WP-023` Bytes that are not text arrive as bytes

| Step | Statement |
| --- | --- |
| Given | bytes that are not valid text |
| When | they are written and read back |
| Then | they arrive unchanged and as bytes |

## `WP-024` Bytes declared as bytes stay bytes

| Step | Statement |
| --- | --- |
| Given | bytes whose holder declares them binary though they read as text |
| When | they are written and read back |
| Then | they are written under the byte-string tag and arrive as bytes |

## `WP-025` A list of nothing crosses

| Step | Statement |
| --- | --- |
| Given | a list carrying no values |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-026` A list of every kind of value crosses

| Step | Statement |
| --- | --- |
| Given | a list carrying one value of each kind the mapping names |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-027` A list within a list crosses

| Step | Statement |
| --- | --- |
| Given | a list nested several times over |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-028` A list crosses at every length tier's bounds

| Step | Statement |
| --- | --- |
| Given | lists at each bound of the two widenings of the list length |
| When | each is written and read back |
| Then | each arrives as it was given |

## `WP-029` A map of nothing crosses

| Step | Statement |
| --- | --- |
| Given | a map carrying no entries |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-030` A map keyed by text crosses

| Step | Statement |
| --- | --- |
| Given | a map whose keys are text |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-031` A map keyed by anything the mapping names crosses too

| Step | Statement |
| --- | --- |
| Given | a map whose keys are numbers and truth values |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-032` A map within a map crosses

| Step | Statement |
| --- | --- |
| Given | a map nested several times over |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-033` A tree mixing references and names throughout crosses

| Step | Statement |
| --- | --- |
| Given | a tree of lists and maps carrying capability references and names at several depths |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-034` Reading past the nesting bound is a wire violation, not a crash

| Step | Statement |
| --- | --- |
| Given | bytes nesting deeper than the wire encodes |
| When | they are read |
| Then | a wire violation is raised that a caller can rescue |

## `WP-035` Writing a value that nests without end is one too

| Step | Statement |
| --- | --- |
| Given | a list holding itself |
| When | it is written |
| Then | a wire violation is raised that a caller can rescue |

## `WP-036` This writer will write nesting its own reader refuses

| Step | Statement |
| --- | --- |
| Given | a value nesting one step past the wire bound |
| When | it is written and the bytes are read back |
| Then | the write succeeds and the read refuses |

## `WP-037` The depth the reader refuses at is the one the wire states

| Step | Statement |
| --- | --- |
| Given | a value nesting exactly to the wire bound |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-038` A name crosses whatever its length

| Step | Statement |
| --- | --- |
| Given | names carrying no characters, single-byte characters, and multibyte characters |
| When | each is written and read back |
| Then | each arrives as it was given |

## `WP-039` A name and the text spelling it stay apart

| Step | Statement |
| --- | --- |
| Given | a name and the text carrying the same characters |
| When | both are written and read back |
| Then | each arrives as its own kind and the two are not equal |

## `WP-040` A name whose bytes are not text is refused

| Step | Statement |
| --- | --- |
| Given | bytes carrying a name whose payload is not valid text |
| When | they are read |
| Then | the encoding is refused |

## `WP-041` A capability reference crosses at its lowest identifier

| Step | Statement |
| --- | --- |
| Given | a capability reference carrying the lowest identifier |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-042` And at its highest

| Step | Statement |
| --- | --- |
| Given | a capability reference carrying the highest identifier |
| When | it is written and read back |
| Then | it arrives as it was given |

## `WP-043` The reserved identifier names no reference

| Step | Statement |
| --- | --- |
| Given | the reserved identifier |
| When | a capability reference is made from it |
| Then | it is refused |

## `WP-044` Nor does one past the highest

| Step | Statement |
| --- | --- |
| Given | an identifier past the highest |
| When | a capability reference is made from it |
| Then | it is refused |

## `WP-045` The reserved identifier is refused off the wire too

| Step | Statement |
| --- | --- |
| Given | bytes carrying a capability reference at the reserved identifier |
| When | they are read |
| Then | a wire violation is raised |

## `WP-046` As is one past the highest

| Step | Statement |
| --- | --- |
| Given | bytes carrying a capability reference past the highest identifier |
| When | they are read |
| Then | a wire violation is raised |

## `WP-047` A reference whose payload is the wrong width is refused by width

| Step | Statement |
| --- | --- |
| Given | bytes carrying a capability reference whose payload is not four bytes |
| When | they are read |
| Then | a wire violation names the width required |

## `WP-048` Bytes carrying nothing at all are truncated

| Step | Statement |
| --- | --- |
| Given | no bytes |
| When | they are read |
| Then | the input is refused as truncated |

## `WP-049` Text promising more than it carries is truncated

| Step | Statement |
| --- | --- |
| Given | bytes declaring a text length the payload does not reach |
| When | they are read |
| Then | the input is refused as truncated |

## `WP-050` A number promising more than it carries is truncated

| Step | Statement |
| --- | --- |
| Given | bytes declaring an eight-byte integer the payload does not reach |
| When | they are read |
| Then | the input is refused as truncated |

## `WP-051` A tag the mapping never uses is a wire violation

| Step | Statement |
| --- | --- |
| Given | bytes opening with the tag reserved as never used |
| When | they are read |
| Then | a wire violation is raised |

## `WP-052` An extension the mapping does not name is one too

| Step | Statement |
| --- | --- |
| Given | bytes carrying an extension code the mapping does not name |
| When | they are read |
| Then | a wire violation is raised |

## `WP-053` Text whose bytes are not text is refused

| Step | Statement |
| --- | --- |
| Given | bytes carrying text whose payload is not valid text |
| When | they are read |
| Then | the encoding is refused |

## `WP-054` A map key is checked the same way

| Step | Statement |
| --- | --- |
| Given | bytes carrying a map whose key is text that is not valid text |
| When | they are read |
| Then | the encoding is refused |

## `WP-055` And so is a map value

| Step | Statement |
| --- | --- |
| Given | bytes carrying a map whose value is text that is not valid text |
| When | they are read |
| Then | the encoding is refused |

## `WP-056` A value the mapping does not name is refused when written

| Step | Statement |
| --- | --- |
| Given | a value of a kind the mapping does not name |
| When | it is written |
| Then | it is refused as a value the mapping does not carry |

## `WP-057` Bytes past one complete value are a wire violation

| Step | Statement |
| --- | --- |
| Given | bytes carrying one complete value followed by another |
| When | they are read |
| Then | a wire violation is raised |

## `WP-058` Nothing takes the byte the wire states

| Step | Statement |
| --- | --- |
| Given | the value that stands for nothing |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-059` A small positive number takes the byte the wire states

| Step | Statement |
| --- | --- |
| Given | a positive number inside the single-byte tier |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-060` A small negative number takes the byte the wire states

| Step | Statement |
| --- | --- |
| Given | a negative number inside the single-byte tier |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-061` Short text takes the bytes the wire states

| Step | Statement |
| --- | --- |
| Given | text short enough for the narrowest text tag |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-062` A short list takes the bytes the wire states

| Step | Statement |
| --- | --- |
| Given | a list short enough for the narrowest list tag |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-063` A name of no length takes the bytes the wire states

| Step | Statement |
| --- | --- |
| Given | a name carrying no characters |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-064` A short name takes the narrowest extension tag

| Step | Statement |
| --- | --- |
| Given | a name short enough for a narrower tag than the widest |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-065` A capability reference takes the bytes the wire states

| Step | Statement |
| --- | --- |
| Given | a capability reference carrying the lowest identifier |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-066` And so does one at the highest identifier

| Step | Statement |
| --- | --- |
| Given | a capability reference carrying the highest identifier |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-067` Text of no length takes the narrowest tag

| Step | Statement |
| --- | --- |
| Given | text carrying no characters |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-068` Bytes of no length take the narrowest tag

| Step | Statement |
| --- | --- |
| Given | bytes carrying nothing |
| When | they are written |
| Then | the bytes are the ones the wire states |

## `WP-069` A list of nothing takes the narrowest tag

| Step | Statement |
| --- | --- |
| Given | a list carrying no values |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-070` A map of nothing takes the narrowest tag

| Step | Statement |
| --- | --- |
| Given | a map carrying no entries |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-071` Zero takes the single-byte tier

| Step | Statement |
| --- | --- |
| Given | zero |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-072` The last positive number in the tier still takes it

| Step | Statement |
| --- | --- |
| Given | the highest positive number the single-byte tier carries |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-073` As does the first negative one

| Step | Statement |
| --- | --- |
| Given | the lowest negative number the single-byte tier carries |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-074` A reader over bytes stands before the first of them

| Step | Statement |
| --- | --- |
| Given | a reader over bytes carrying a value |
| When | it is asked where it stands |
| Then | it stands at the beginning and is not at its end |

## `WP-075` A reader over no bytes is already at its end

| Step | Statement |
| --- | --- |
| Given | a reader over no bytes |
| When | it is asked whether it is at its end |
| Then | it is |

## `WP-076` A writer that has written nothing carries nothing

| Step | Statement |
| --- | --- |
| Given | a writer nothing has been written to |
| When | its bytes are taken |
| Then | there are none |

## `WP-077` A buffer holding one complete value is read as that value

| Step | Statement |
| --- | --- |
| Given | bytes carrying exactly one value |
| When | they are read as a sole value |
| Then | the value arrives as it was written |

## `WP-078` A list declaring more than the bytes carry is refused before anything is allocated

| Step | Statement |
| --- | --- |
| Given | bytes declaring a list of more elements than could follow |
| When | they are read |
| Then | the input is refused as truncated |

## `WP-079` A map declaring more than the bytes carry is refused the same way

| Step | Statement |
| --- | --- |
| Given | bytes declaring a map of more entries than could follow |
| When | they are read |
| Then | the input is refused as truncated |

## `WP-080` A number past the signed ceiling keeps its unsigned reading

| Step | Statement |
| --- | --- |
| Given | a number above the highest signed number |
| When | it is read |
| Then | it arrives unsigned |

## `WP-081` A number inside the signed range comes back signed

| Step | Statement |
| --- | --- |
| Given | an unsigned number no higher than the highest signed one |
| When | it is written and read back |
| Then | it arrives signed |

## `WP-082` A float written at the narrower width is read

| Step | Statement |
| --- | --- |
| Given | bytes carrying a float at the narrower of the two widths |
| When | they are read |
| Then | the float arrives with its value intact |

## `WP-083` Truth takes the byte the wire states

| Step | Statement |
| --- | --- |
| Given | truth |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-084` And falsity the byte the wire states

| Step | Statement |
| --- | --- |
| Given | falsity |
| When | it is written |
| Then | the bytes are the ones the wire states |

## `WP-085` The extension codes are the ones the wire states

| Step | Statement |
| --- | --- |
| Given | the codes this mapping writes its two extensions under |
| When | they are read |
| Then | each is the code the wire states |

## `WP-086` The highest reference identifier is the one the wire states

| Step | Statement |
| --- | --- |
| Given | the ceiling this mapping holds capability references to |
| When | it is read |
| Then | it is the one the wire states |

## `WP-087` The writer that carries the bound refuses to write past it

| Step | Statement |
| --- | --- |
| Given | a value nesting one step past the wire bound |
| When | it is written by the writer that carries the bound |
| Then | a wire violation is raised rather than bytes its own reader would refuse |

## `WP-088` Each integer tier begins at the number the wire states

| Step | Statement |
| --- | --- |
| Given | each bound of the integer tiers, on both sides of zero |
| When | each is written |
| Then | each takes the narrowest tier that holds it |

## `WP-089` The widest unsigned number takes the widest tier

| Step | Statement |
| --- | --- |
| Given | the highest unsigned number |
| When | it is written |
| Then | it takes the widest tier |

## `WP-090` Each text tier begins at the length the wire states

| Step | Statement |
| --- | --- |
| Given | text at each bound of the text tiers |
| When | each is written |
| Then | each takes the narrowest tier that holds it |

## `WP-091` Each bytes tier begins at the length the wire states

| Step | Statement |
| --- | --- |
| Given | bytes at each bound of the bytes tiers |
| When | each is written |
| Then | each takes the narrowest tier that holds it |

## `WP-092` Each list tier begins at the count the wire states

| Step | Statement |
| --- | --- |
| Given | a list at each bound of the list tiers |
| When | each is written |
| Then | each takes the narrowest tier that holds it |

## `WP-093` Each map tier begins at the count the wire states

| Step | Statement |
| --- | --- |
| Given | a map at each bound of the map tiers |
| When | each is written |
| Then | each takes the narrowest tier that holds it |

## `WP-094` A keyword name reaches the wire as a name

| Step | Statement |
| --- | --- |
| Given | an invocation carrying a keyword argument |
| When | the payload is written |
| Then | the key rides as a name rather than as text |

## `WP-095` The mapping names a closed set of value kinds

| Step | Statement |
| --- | --- |
| Given | the kinds this mapping carries |
| When | the set is read |
| Then | it holds each of them and nothing else |

## `WP-096` A reference the writer would never emit is refused when written

| Step | Statement |
| --- | --- |
| Given | a capability reference carrying the reserved identifier, and one past the highest |
| When | each is written |
| Then | each is refused rather than emitted for a reader to reject |

## `WP-097` A number promising more than it carries is truncated at any width

| Step | Statement |
| --- | --- |
| Given | bytes declaring a number of a width the payload does not reach |
| When | they are read |
| Then | the input is refused as truncated |
