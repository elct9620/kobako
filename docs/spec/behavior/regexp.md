# Regexp

The pattern object the guest compiles and matches with, and what it refuses to read.

## Includes

- `test/e2e/regexp/test_regexp_methods.rb`
- `test/e2e/regexp/test_match_operand.rb`
- `test/e2e/regexp/test_match_globals.rb`
- `test/e2e/regexp/test_match_position.rb`
- `test/e2e/regexp/test_match_block.rb`
- `test/e2e/regexp/test_kernel.rb`
- `test/e2e/regexp/test_compile_cache.rb`
- `test/e2e/regexp/test_regexp_inspect.rb`
- `test/e2e/regexp/test_regexp_to_s.rb`
- `test/e2e/regexp/test_object_copy.rb`
- `test/e2e/regexp/test_pattern_errors.rb`
- `test/e2e/regexp/test_utf8.rb`
- `test/e2e/regexp/test_non_utf8.rb`
- `test/e2e/regexp/test_unicode_gate.rb`

### Why these scenarios

Matching happens entirely inside the guest, so what is observable is what a pattern answers and what it refuses. Offsets are bytes throughout, and the scenarios say so wherever a character count would read the same on ASCII and differently on anything else.

Text that is not text is refused rather than read as empty. An empty subject reports no match for every pattern and an empty pattern matches everywhere, so both failures would be silent and both would be wrong; the refusal is witnessed at each place such bytes can enter, and once more where ordinary text still works, so the boundary is a boundary and not a regression.

Memoizing a compiled pattern is meant to be invisible, so its scenarios assert results rather than timings: distinct objects, options as part of the identity, and correct matching past the memo's capacity.

## `RX-001` The match operator answers where the match starts

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a pattern against a String with the match operator |
| Then | it answers the byte index of the first match |

## `RX-002` No match is nothing, not a sentinel number

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a pattern that does not occur |
| Then | the match operator answers nothing |

## `RX-003` The predicate answers whether, not where

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code asks a pattern whether it matches a String |
| Then | it answers true |

## `RX-004` Case equality matches for a case expression

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code tests a pattern against a String with case equality |
| Then | it answers true |

## `RX-005` Case equality is false where the pattern does not occur

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code tests a pattern that does not occur with case equality |
| Then | it answers false |

## `RX-006` A pattern remembers the text it was written as

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a pattern's source |
| Then | it answers the text the pattern was written as |

## `RX-007` A case-insensitive pattern says so

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code asks a case-insensitive pattern whether it folds case |
| Then | it answers true |

## `RX-008` A pattern without the flag says so too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code asks a pattern carrying no case flag whether it folds case |
| Then | it answers false |

## `RX-009` Escaping quotes what would otherwise be syntax

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code escapes a String holding pattern metacharacters |
| Then | each metacharacter comes back backslash-quoted |

## `RX-010` Escaping leaves alone what needs no quoting

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code escapes a String holding a slash and a vertical tab |
| Then | the slash stays as it was while the vertical tab is quoted |

## `RX-011` Compiling by name builds the same pattern as writing one

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code compiles a pattern from a String and matches with it |
| Then | it answers the matched substring |

## `RX-012` A pattern built at run time captures like any other

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code builds a pattern with a group from a String and matches with it |
| Then | it answers the captured substring |

## `RX-013` An option given at construction takes effect

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code builds a pattern with the case-insensitive option and matches text of the other case |
| Then | it matches |

## `RX-014` The options a pattern reports are the language's bits

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a case-insensitive pattern's options |
| Then | it answers the case-insensitive bit |

## `RX-015` Several options report as their combination

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads the options of a pattern carrying two flags |
| Then | it answers those two bits combined |

## `RX-016` A pattern maps each capture name to its group numbers

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads the named captures of a pattern with two named groups |
| Then | each name maps to the group numbers carrying it |

## `RX-017` A pattern with no names maps nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads the named captures of a pattern with unnamed groups |
| Then | the map is empty |

## `RX-018` A pattern lists its capture names as written

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads the names of a pattern with two named groups |
| Then | they come back in declaration order |

## `RX-019` A pattern with no names lists nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads the names of a pattern with unnamed groups |
| Then | the list is empty |

## `RX-020` The predicate refuses a subject that is not text

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code asks a pattern whether it matches an Integer |
| Then | a type error is raised |

## `RX-021` Matching refuses a subject that is not text

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a pattern against an Integer |
| Then | a type error is raised |

## `RX-022` The match operator refuses one too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code applies the match operator to an Integer |
| Then | a type error is raised |

## `RX-023` Case equality answers false rather than raising

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code tests a pattern against an Integer with case equality |
| Then | it answers false |

## `RX-024` Nothing is no match, not an empty subject

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a pattern that would match emptiness against nothing |
| Then | it answers nothing |

## `RX-025` The predicate reads nothing as no match too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code asks a pattern whether it matches nothing |
| Then | it answers false |

## `RX-026` A Symbol is text enough to match against

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code tests a pattern against a Symbol with case equality |
| Then | it matches that Symbol's name |

## `RX-027` A String matches against a pattern and answers its match

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a String against a pattern with capture groups |
| Then | it answers a match carrying the whole match and each capture |

## `RX-028` A String is not a pattern, even where one is expected

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code asks a String whether it matches another String |
| Then | a type error is raised |

## `RX-029` Nor is anything else

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code asks a String whether it matches an Integer |
| Then | a type error is raised |

## `RX-030` A numbered global holds its capture

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| Given | guest code that matched a pattern with one group |
| When | it reads the first numbered global |
| Then | it holds that group's capture |

## `RX-031` The match global holds the match itself

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| Given | guest code that matched a pattern |
| When | it reads the match global |
| Then | it holds that match |

## `RX-032` The whole-match global holds what was matched

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| Given | guest code that matched a pattern |
| When | it reads the whole-match global |
| Then | it holds the matched substring |

## `RX-033` The surrounding globals hold what the match sat between

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| Given | guest code that matched a pattern inside a longer String |
| When | it reads the before-match and after-match globals |
| Then | they hold the text on either side of the match |

## `RX-034` The last-group global holds the highest group that took part

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| Given | guest code that matched a pattern with two groups |
| When | it reads the last-group global |
| Then | it holds the highest-numbered group that matched |

## `RX-035` A pattern with no groups leaves that global empty

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| Given | guest code that matched a pattern with no groups |
| When | it reads the last-group global |
| Then | it holds nothing |

## `RX-036` A numbered global follows each iteration of a substitution block

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes with a block reading the first numbered global |
| Then | each iteration reads its own capture |

## `RX-037` The last match is readable as a value, not only as a global

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| Given | guest code that matched a pattern |
| When | it asks the pattern class for the last match |
| Then | it answers that match |

## `RX-038` Before anything matched there is no last match

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code asks for the last match before matching anything |
| Then | it answers nothing |

## `RX-039` A saved match can be put back

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| Given | guest code that saved one match and then ran another |
| When | it assigns the saved match back and reads it |
| Then | it answers the saved match |

## `RX-040` Putting a match back brings its numbered globals with it

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| Given | guest code that saved one match and then ran another |
| When | it assigns the saved match back and reads the first numbered global |
| Then | it holds the saved match's capture |

## `RX-041` Clearing the match clears what reads through it

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| Given | guest code that matched a pattern with one group |
| When | it clears the last match and reads the first numbered global |
| Then | it holds nothing |

## `RX-042` A position starts the search where it says

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a pattern from a byte position past an earlier occurrence |
| Then | it answers the occurrence at or after that position |

## `RX-043` A position past the end matches nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches from a position beyond the subject's end |
| Then | it answers nothing |

## `RX-044` A negative position counts back from the end

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches from a negative position |
| Then | the search starts that far back from the end |

## `RX-045` A negative position past the start matches nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches from a negative position further back than the subject is long |
| Then | it answers nothing |

## `RX-046` A position at the end can still match emptiness

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a zero-width pattern from the subject's length |
| Then | the match begins at that position |

## `RX-047` The predicate reads a position the same way

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code asks whether a pattern matches from a position beyond the end |
| Then | it answers false |

## `RX-048` Matching hands the match to a block and answers what the block said

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a pattern with a block |
| Then | the block receives the match and the call answers the block's value |

## `RX-049` A block that had nothing to receive is not run

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a pattern that does not occur with a block |
| Then | it answers nothing without running the block |

## `RX-050` A String passes a block through to the pattern

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a String against a pattern with a block |
| Then | the block receives the match |

## `RX-051` A receiver that is not text answers nothing rather than raising

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code applies the match operator with an Integer on the left |
| Then | it answers nothing |

## `RX-052` A Symbol on the left answers nothing too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code applies the match operator with a Symbol on the left |
| Then | it answers nothing |

## `RX-053` A String on the left still matches

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code applies the match operator with a String on the left and a pattern on the right |
| Then | it answers the byte offset of the match |

## `RX-054` A String on the right of a String is not a pattern

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code applies the match operator between two Strings |
| Then | a type error is raised |

## `RX-055` Anything else on the right answers nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code applies the match operator with a String on the left and an Integer on the right |
| Then | it answers nothing |

## `RX-056` Reusing a pattern's engine does not reuse the pattern

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code compares two evaluations of the same pattern literal for identity |
| Then | they are distinct objects |

## `RX-057` The same text under different options is a different pattern

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches the same source with and without the case-insensitive option |
| Then | each matches on its own terms |

## `RX-058` More patterns than the memo holds still match correctly

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code builds and matches a hundred distinct patterns in turn |
| Then | each matches its own subject and no other |

## `RX-059` A pattern matched in a hot loop counts the same either way

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches one literal a thousand times |
| Then | it reports a thousand hits |

## `RX-060` Inspecting shows the source between slashes with its flags

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code inspects a case-insensitive pattern |
| Then | it renders the source between slashes followed by its flags |

## `RX-061` A slash in the source is escaped for the rendering

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code inspects a pattern whose source holds a slash |
| Then | the slash renders escaped |

## `RX-062` A whitespace control renders as itself

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code inspects a pattern whose source holds a newline |
| Then | the newline renders literally |

## `RX-063` Any other control renders as its hexadecimal escape

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code inspects a pattern whose source holds an escape control byte |
| Then | it renders as an uppercase hexadecimal escape |

## `RX-064` Multibyte text renders as text

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code inspects a pattern whose source holds multibyte characters |
| Then | they render unescaped |

## `RX-065` The string form spells out every flag, on or off

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code converts a pattern carrying no flags to a String |
| Then | every flag appears in the disabled block |

## `RX-066` A flag that is on appears on the enabled side

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code converts a case-insensitive pattern to a String |
| Then | that flag appears enabled and the others disabled |

## `RX-067` With every flag on there is nothing to disable

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code converts a pattern carrying every flag to a String |
| Then | the disabled block is omitted |

## `RX-068` An inline group spanning the whole source is lifted into the flags

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code converts a pattern whose whole source is one inline-flag group to a String |
| Then | the inline flag appears in the outer flags |

## `RX-069` A flagless group spanning the whole source is dropped

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code converts a pattern whose whole source is one flagless group to a String |
| Then | the group is dropped and its body kept |

## `RX-070` A lifted flag joins the ones already there

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code converts a pattern with both an inline flag and an outer option to a String |
| Then | both appear in the outer flags |

## `RX-071` A group that does not span the source stays where it is

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code converts a pattern whose inline group covers only part of the source to a String |
| Then | the group renders verbatim inside the body |

## `RX-072` Only the outermost group is lifted

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code converts a pattern whose whole-span group contains another to a String |
| Then | the inner group remains as the body |

## `RX-073` A copied pattern is a working pattern

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code duplicates a pattern and uses the copy |
| Then | the copy carries the source and options and matches |

## `RX-074` Cloning carries the same

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code clones a pattern and uses the clone |
| Then | the clone carries the source and options and matches |

## `RX-075` A copied match carries the whole snapshot

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code duplicates a match and reads the copy |
| Then | the copy carries the groups, the subject and the pattern it came from |

## `RX-076` A pattern that cannot compile fails the invocation

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code builds a pattern from unbalanced source and leaves the failure unrescued |
| Then | `Kobako::SandboxError` is raised |

## `RX-077` A shape that costs dearly still answers where the language answers

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a nested-quantifier backreference against a subject that does not match |
| Then | it answers nothing rather than failing |

## `RX-078` A match the bound stops fails the invocation

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code runs a match past the engine's bound and leaves the failure unrescued |
| Then | `Kobako::SandboxError` is raised |

## `RX-079` The diagnostic names the pattern

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code rescues a match-time engine failure and reads its message |
| Then | the message names the pattern source |

## `RX-080` The diagnostic does not quote the subject

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code rescues a match-time engine failure and reads its message |
| Then | the message does not carry the subject text |

## `RX-081` A pattern failure is an ordinary error the guest can rescue

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code asks whether the pattern error class descends from the standard error class |
| Then | it does |

## `RX-082` A multibyte pattern slices multibyte text

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code slices a String with a multibyte literal pattern |
| Then | it answers the matching substring |

## `RX-083` Offsets count bytes, not characters

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches inside a String holding multibyte characters |
| Then | it answers the byte offset |

## `RX-084` A multibyte capture survives the crossing to the host

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code returns a multibyte capture as an Array |
| Then | the host receives the substrings unchanged |

## `RX-085` A shorthand class outside a character class is the language's ASCII one

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a non-ASCII digit against a negated digit shorthand |
| Then | it matches |

## `RX-086` The same shorthand inside a character class is the engine's Unicode one

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a non-ASCII digit against a negated digit shorthand inside a character class |
| Then | it does not match |

## `RX-087` A subject that is not text is refused, not read as empty

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches against a String whose bytes are not text |
| Then | an argument error is raised |

## `RX-088` Substitution refuses such a subject too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes within a String whose bytes are not text |
| Then | an argument error is raised |

## `RX-089` A pattern source that is not text is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code builds a pattern from a String whose bytes are not text |
| Then | an argument error is raised |

## `RX-090` A replacement that is not text is refused

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes with a replacement whose bytes are not text |
| Then | an argument error is raised |

## `RX-091` Escaping refuses one as well

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code escapes a String whose bytes are not text |
| Then | an argument error is raised |

## `RX-092` The refusal reaches no further than the bytes that caused it

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches against an ordinary text subject |
| Then | it answers the byte offset of the match |

## `RX-093` Without the Unicode build a case-insensitive pattern is refused, not silently narrowed

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp Guest Binary built without Unicode support |
| When | guest code compiles and uses a case-insensitive pattern |
| Then | a pattern error is raised |

## `RX-094` ASCII matching stands without the Unicode build

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp Guest Binary built without Unicode support |
| When | guest code matches an ASCII digit shorthand |
| Then | it answers the matched substring |
