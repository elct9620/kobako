# Guest IO

The two descriptors guest code may write to, and the writing surface it reaches them through.

## Why these scenarios

Guest code can write to two descriptors and no others. The constraint is witnessed twice — where a stream is opened and where it is written through — because the descriptor a stream carries is guest-mutable, which makes the opening check a courtesy on its own.

The rest is fidelity. Bytes go out as bytes: inline and heap strings alike, zero bytes included, a String unchanged and anything else through its string form. The supplementary surface is spelled out one member at a time because a script written against the mruby IO it mirrors expects each of them, and a single collective assertion would let any one of them drift.

Which channel bytes land in, where they stop, and what survives a failed run are the host end of this surface and belong with the Sandbox behaviors.

## IO-001 — Only the captured descriptors may be opened

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code constructs an IO over a descriptor outside the captured pair |
| Then | the invocation fails with a message naming the descriptor constraint |

## IO-002 — Only writing is offered

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code constructs an IO in a mode other than write |
| Then | the invocation fails with a message naming the mode constraint |

## IO-003 — The descriptor is checked where it is used, not only where it was given

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| Given | guest code that reassigned an opened IO's descriptor to another number |
| When | it writes through that IO |
| Then | the invocation fails with a message naming the descriptor constraint |

## IO-004 — The standard output stream reports its descriptor

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code reads the standard output stream's descriptor number |
| Then | it is 1 |

## IO-005 — The standard error stream reports its descriptor

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code reads the standard error stream's descriptor number |
| Then | it is 2 |

## IO-006 — Appending answers the stream, so it chains

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code appends to the standard output stream |
| Then | the append answers that same stream |

## IO-007 — A captured stream is not a terminal

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code asks the standard output stream whether it is a terminal |
| Then | it is not |

## IO-008 — A captured stream is synchronous to begin with

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code reads the standard output stream's sync flag |
| Then | it is true |

## IO-009 — Setting the sync flag answers what was assigned

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code assigns the standard output stream's sync flag |
| Then | the assignment answers the value assigned |

## IO-010 — The sync flag keeps what it was set to

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| Given | guest code that turned the standard output stream's sync flag off |
| When | it reads the flag back |
| Then | it is false |

## IO-011 — Flushing answers the stream

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code flushes the standard output stream |
| Then | the flush answers that same stream |

## IO-012 — A captured stream is open for as long as the invocation runs

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code asks the standard output stream whether it is closed |
| Then | it is not |

## IO-013 — The integer conversion is the descriptor number

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code converts the standard output stream to an Integer |
| Then | it is the stream's descriptor number |

## IO-014 — Appended bytes are captured like written ones

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code appends bytes to the standard output stream |
| Then | those bytes appear in the output capture |

## IO-015 — A short string reaches the capture whole

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code prints a string short enough to be stored inline |
| Then | the capture holds it byte for byte |

## IO-016 — A long string reaches the capture whole

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code prints a string too long to be stored inline |
| Then | the capture holds it byte for byte |

## IO-017 — A payload is bytes, not text up to the first zero

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code prints a string containing a zero byte |
| Then | the capture holds every byte including the zero |

## IO-018 — A string is written as it stands

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code prints a String |
| Then | the capture holds its bytes unchanged |

## IO-019 — Anything else is written as its string form

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code prints a value that is not a String |
| Then | the capture holds that value's string form |

## IO-020 — Formatting applies width and precision

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code formats a number with width and precision specifiers |
| Then | the invocation answers the formatted String |

## IO-021 — A String formats against a list of arguments

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code formats a String against an Array of arguments |
| Then | the invocation answers the interpolated String |

## IO-022 — Formatted printing reaches the capture

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code prints with a format string |
| Then | the capture holds the formatted bytes |

## IO-023 — Putting a character number writes that byte

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code puts a character by its number |
| Then | the capture holds that one byte |

## IO-024 — Putting a character does not touch the error channel

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code puts a character |
| Then | the error capture is empty |

## IO-025 — A character number above a byte is taken modulo a byte

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code puts a character by a number larger than a byte |
| Then | the capture holds the low byte of that number |

## IO-026 — Putting a character answers nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code's last expression puts a character |
| Then | the invocation answers nothing |

## IO-027 — The writing delegators are not callable on a receiver

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code calls a writing delegator with an explicit receiver |
| Then | the invocation fails with the guest's no-method error |

## IO-028 — A refused delegator writes nothing on its way out

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code calls a writing delegator with an explicit receiver |
| Then | the output capture is empty |

## IO-029 — Putting a string writes its first character

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code puts a multi-character String |
| Then | the capture holds only its first character |

## IO-030 — Inspecting writes the inspect form, not the string form

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code inspects a Hash to the output stream |
| Then | the capture holds the Hash's inspect form |

## IO-031 — A long argument list is written to the end

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code writes a hundred and fifty arguments in one call |
| Then | the capture holds a line for every one of them |

## IO-032 — A list is flattened by what it is, not by its class

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code writes an instance of an Array subclass |
| Then | the capture holds one line per element |
