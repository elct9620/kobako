# Guest IO

The two descriptors guest code may write to, and the writing surface it reaches them through.

### Why these scenarios

Guest code can write to the two captured descriptors and the files it opened, and no others. The constraint is witnessed twice: where a stream is opened and where it is written through. A copy of a stream keeps its descriptor after the original closes it. The opening check alone is therefore only a courtesy.

The rest is fidelity. Bytes go out as bytes: inline and heap strings alike, zero bytes included. A String goes out unchanged, and anything else goes out through its string form.

The supplementary surface is spelled out one member at a time. A script written against the mruby IO it mirrors expects each member. A single collective assertion would let any one of them drift.

The host end of this surface belongs with the Sandbox behaviors. It covers which channel bytes land in, where they stop, and what survives a failed run.

## Includes

- `test/e2e/test_io_write.rb`
- `test/e2e/test_io_kernel.rb`
- `test/e2e/test_io_state.rb`

## `IO-001` Only the captured descriptors and the guest's own files may be opened

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code constructs an IO over a descriptor it neither captures nor opened |
| Then | the invocation fails with a message naming the descriptor constraint |

## `IO-002` Only writing is offered on a captured descriptor

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code constructs an IO over a captured descriptor in a mode other than write |
| Then | the invocation fails with a message naming the mode constraint |

## `IO-003` The descriptor is checked where it is used, not only where it was given

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| Given | guest code that copied an opened file and closed the original |
| When | it writes through the copy |
| Then | the invocation fails with a message naming the descriptor constraint |

## `IO-004` The standard output stream reports its descriptor

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code reads the standard output stream's descriptor number |
| Then | it is 1 |

## `IO-005` The standard error stream reports its descriptor

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code reads the standard error stream's descriptor number |
| Then | it is 2 |

## `IO-006` Appending answers the stream, so it chains

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code appends to the standard output stream |
| Then | the append answers that same stream |

## `IO-007` A captured stream is not a terminal

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code asks the standard output stream whether it is a terminal |
| Then | it is not |

## `IO-008` A captured stream is synchronous to begin with

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code reads the standard output stream's sync flag |
| Then | it is true |

## `IO-009` Setting the sync flag answers what was assigned

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code assigns the standard output stream's sync flag |
| Then | the assignment answers the value assigned |

## `IO-010` The sync flag keeps what it was set to

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| Given | guest code that turned the standard output stream's sync flag off |
| When | it reads the flag back |
| Then | it is false |

## `IO-011` Flushing answers the stream

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code flushes the standard output stream |
| Then | the flush answers that same stream |

## `IO-012` A captured stream is open for as long as the invocation runs

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code asks the standard output stream whether it is closed |
| Then | it is not |

## `IO-013` The integer conversion is the descriptor number

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code converts the standard output stream to an Integer |
| Then | it is the stream's descriptor number |

## `IO-014` Appended bytes are captured like written ones

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code appends bytes to the standard output stream |
| Then | those bytes appear in the output capture |

## `IO-015` A short string reaches the capture whole

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code prints a string short enough to be stored inline |
| Then | the capture holds it byte for byte |

## `IO-016` A long string reaches the capture whole

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code prints a string too long to be stored inline |
| Then | the capture holds it byte for byte |

## `IO-017` A payload is bytes, not text up to the first zero

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code prints a string containing a zero byte |
| Then | the capture holds every byte including the zero |

## `IO-018` A string is written as it stands

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code prints a String |
| Then | the capture holds its bytes unchanged |

## `IO-019` Anything else is written as its string form

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code prints a value that is not a String |
| Then | the capture holds that value's string form |

## `IO-020` Formatting applies width and precision

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code formats a number with width and precision specifiers |
| Then | the invocation answers the formatted String |

## `IO-021` A String formats against a list of arguments

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code formats a String against an Array of arguments |
| Then | the invocation answers the interpolated String |

## `IO-022` Formatted printing reaches the capture

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code prints with a format string |
| Then | the capture holds the formatted bytes |

## `IO-023` Putting a character number writes that byte

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code puts a character by its number |
| Then | the capture holds that one byte |

## `IO-024` Putting a character does not touch the error channel

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code puts a character |
| Then | the error capture is empty |

## `IO-025` A character number above a byte is taken modulo a byte

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code puts a character by a number larger than a byte |
| Then | the capture holds the low byte of that number |

## `IO-026` Putting a character answers nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code's last expression puts a character |
| Then | the invocation answers nothing |

## `IO-027` The writing delegators are not callable on a receiver

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code calls a writing delegator with an explicit receiver |
| Then | the invocation fails with the guest's no-method error |

## `IO-028` A refused delegator writes nothing on its way out

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code calls a writing delegator with an explicit receiver |
| Then | the output capture is empty |

## `IO-029` Putting a string writes its first character

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code puts a multi-character String |
| Then | the capture holds only its first character |

## `IO-030` Inspecting writes the inspect form, not the string form

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code inspects a Hash to the output stream |
| Then | the capture holds the Hash's inspect form |

## `IO-031` A long argument list is written to the end

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code writes a hundred and fifty arguments in one call |
| Then | the capture holds a line for every one of them |

## `IO-032` A list is flattened by what it is, not by its class

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code writes an instance of an Array subclass |
| Then | the capture holds one line per element |

## `IO-033` Putting nothing writes a line break

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code puts to the output stream with no arguments |
| Then | the capture holds a single line break |

## `IO-034` Writing several values writes each in order

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code writes several values to the output stream in one call |
| Then | the capture holds each value's string form in the order given |

## `IO-035` A mode is a String or an Integer of flags

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code constructs an IO with a Symbol naming the write mode |
| Then | a type error is raised |

## `IO-036` A value written in one call is let go once it is written

| Attribute | Value |
| --- | --- |
| unverifiable | the arena grows on demand, so holding shows only in the memory peak |
| refines | `IO-034` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code writes many values to the output stream in one call |
| Then | each value's string form is released for collection once it is written |

## `IO-037` A stream shows no instance variable of its own

| Attribute | Value |
| --- | --- |
| relates | `IO-003` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox with a Mount |
| When | guest code lists the instance variables of the output stream and of an opened file |
| Then | both lists are empty |

## `IO-038` The sync flag reads back as the truth of what was set

| Attribute | Value |
| --- | --- |
| refines | `IO-010` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| Given | guest code that set the standard output stream's sync flag to a truthy value other than true |
| When | it reads the flag back |
| Then | it is true |

## `IO-039` A character number past 64 bits raises RangeError

| Attribute | Value |
| --- | --- |
| refines | `IO-025` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox |
| When | guest code puts a character by a number past 64 bits, through `Kernel` and through `$stdout` |
| Then | each raises `RangeError` and the capture holds nothing |

## `IO-040` An IO over a file this guest opened reads its mode as File.open does

| Attribute | Value |
| --- | --- |
| refines | `IO-035` |
| relates | `FL-026`, `FL-027` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose script opened a file beneath a read-write Mount |
| When | the script wraps that file's descriptor with `File::RDWR`, then with `zz` |
| Then | the first IO is built, and the second raises `ArgumentError` |

## `IO-041` An IO asking an access its file lacks raises EINVAL

| Attribute | Value |
| --- | --- |
| refines | `IO-040` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose script opened a file only to read |
| When | the script wraps that file's descriptor in a mode that writes |
| Then | the wrap raises `Errno::EINVAL` |
