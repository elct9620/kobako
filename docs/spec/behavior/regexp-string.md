# Regexp over String

What a String does when a pattern is handed to it — searching, splitting, substituting, and rewriting in place.

## Why these scenarios

A String handed a pattern searches, splits, substitutes or rewrites itself. Each of those takes the same pattern and answers differently, so each is witnessed separately, and where a String argument reaches the same method the scenario says the ordinary behavior still stands — the pattern surface layers over the language's own rather than replacing it.

Splitting and scanning disagree deliberately about a group that did not take part: splitting drops it, scanning keeps the hole. Both are witnessed on the same pattern, because the contrast is the contract.

Replacement text is a small language of its own, so its scenarios cover what expands, what stays literal, and what is refused — a name no group carries, and a name marker with no name behind it.

## RX-118 — Matching a String answers a match with its captures

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code matches a String against a pattern with groups |
| Then | it answers a match carrying those captures |

## RX-119 — Global substitution replaces every occurrence

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes globally with a replacement String |
| Then | every match is replaced |

## RX-120 — A block decides each global replacement

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes globally with a block |
| Then | each match is replaced by that iteration's block result |

## RX-121 — Single substitution replaces only the first

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes once with a replacement String |
| Then | only the first match is replaced |

## RX-122 — Scanning a group-less pattern collects the matches

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code scans with a pattern carrying no groups |
| Then | it collects each matched substring |

## RX-123 — Scanning a pattern with groups collects them per match

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code scans with a pattern carrying groups |
| Then | it collects one group list per match |

## RX-124 — Splitting divides on each match

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code splits a String on a pattern |
| Then | it answers the fields between the matches |

## RX-125 — A capturing group in the separator becomes a field

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code splits on a pattern carrying a group |
| Then | each captured substring is interleaved among the fields |

## RX-126 — A positive limit stops splitting and keeps the rest whole

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code splits with a positive limit |
| Then | the remainder stays as the last field |

## RX-127 — A negative limit keeps the trailing empty fields

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code splits with a negative limit on a String ending in separators |
| Then | the trailing empty fields are kept |

## RX-128 — Searching a String answers a byte offset

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code searches a String for a pattern |
| Then | it answers the byte offset of the first match |

## RX-129 — Searching can start from a position

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code searches for a pattern from a byte position |
| Then | it answers the first match at or after that position |

## RX-130 — A negative search position counts from the end

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code searches for a pattern from a negative position |
| Then | the search starts that far back from the end |

## RX-131 — A search that finds nothing from there answers nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code searches from a position past every occurrence |
| Then | it answers nothing |

## RX-132 — A position inside a character snaps to that character's start

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code searches from a byte position inside a multibyte character |
| Then | the search starts at that character's boundary |

## RX-133 — Slicing by a pattern answers what matched

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code slices a String with a pattern |
| Then | it answers the matched substring |

## RX-134 — Slicing by a pattern and a group answers that capture

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code slices a String with a pattern and a group index |
| Then | it answers that capture |

## RX-135 — A block decides the single replacement too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes once with a block |
| Then | the first match is replaced by the block's result |

## RX-136 — Scanning with a block hands each match over

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code scans with a block |
| Then | the block receives each match in turn |

## RX-137 — Splitting on a String is still the ordinary split

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code splits a String on another String |
| Then | it answers what the language's own split answers |

## RX-138 — Splitting drops a group that did not take part

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code splits on a pattern whose optional group did not match |
| Then | no empty placeholder appears among the fields |

## RX-139 — Scanning keeps the same group as nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code scans with a pattern whose optional group did not match |
| Then | that position in the group list holds nothing |

## RX-140 — A zero-width separator does not open with an empty field

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code splits a String on a zero-width pattern |
| Then | the fields are its characters with no leading empty one |

## RX-141 — A zero-width split counts real splits against the limit

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code splits on a zero-width pattern with a positive limit |
| Then | it stops after that many splits and keeps the remainder |

## RX-142 — An empty field between two separators is a field

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code splits a String holding two adjacent separators |
| Then | the empty field between them is kept |

## RX-143 — A numbered backreference in the replacement expands

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes globally with a replacement naming a group by number |
| Then | each replacement carries that group's capture |

## RX-144 — A named backreference expands too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes globally with a replacement naming a group by name |
| Then | each replacement carries that group's capture |

## RX-145 — Single substitution expands them as well

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes once with a replacement carrying a backreference |
| Then | the replacement carries that group's capture |

## RX-146 — A Hash replacement looks each whole match up

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes globally with a Hash replacement |
| Then | each match is replaced by the value it maps to |

## RX-147 — An escape nobody defined stays as it was written

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes with a replacement carrying an unrecognised escape |
| Then | it appears as its two literal characters |

## RX-148 — A named backreference to a group that does not exist is an error

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes with a replacement naming a capture the pattern does not declare |
| Then | an index error is raised |

## RX-149 — A malformed named backreference is a pattern error

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes with a replacement whose name marker carries no name |
| Then | a pattern error is raised |

## RX-150 — The zeroth backreference is the whole match

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes with a replacement naming group zero |
| Then | each replacement carries the whole match |

## RX-151 — A replacement argument outranks a block

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes globally with both a replacement String and a block |
| Then | the replacement String is used |

## RX-152 — Assigning through a pattern overwrites what matched

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code assigns into a String through a pattern |
| Then | the matched region is replaced in place |

## RX-153 — Assigning through a pattern and a group overwrites that group

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code assigns into a String through a pattern and a group index |
| Then | that group's region is replaced in place |

## RX-154 — Assigning through a String is still the ordinary assignment

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code assigns into a String through another String |
| Then | it behaves as the language's own assignment does |

## RX-155 — Assigning through indices is too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code assigns into a String through Integer arguments |
| Then | it behaves as the language's own assignment does |

## RX-156 — Assigning through a pattern that does not match is an error

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code assigns into a String through a pattern that does not occur |
| Then | an index error is raised |

## RX-157 — Cutting by a pattern answers what was cut and removes it

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code cuts a match out of a String |
| Then | it answers the matched substring and the String no longer holds it |

## RX-158 — Cutting a pattern that does not match changes nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code cuts by a pattern that does not occur |
| Then | it answers nothing and the String is unchanged |

## RX-159 — Cutting leaves the last match as its own

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads the last match after cutting by a pattern |
| Then | it holds the cut's own match |

## RX-160 — Cutting by an index is still the ordinary cut

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code cuts a String by an Integer index |
| Then | that character is removed |

## RX-161 — Cutting by a start and a length is too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code cuts a String by an Integer start and length |
| Then | that range is removed |

## RX-162 — And cutting by a String

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code cuts a String by another String |
| Then | its first occurrence is removed |

## RX-163 — A raise inside a scan block reaches the caller

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code scans with a block that raises and rescues around the call |
| Then | the block's exception is what it rescues |

## RX-164 — A raise inside a substitution block reaches the caller too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes with a block that raises and rescues around the call |
| Then | the block's exception is what it rescues |

## RX-165 — Global substitution with nothing to substitute asks for an enumerator

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes globally with neither a block nor a replacement |
| Then | the invocation fails naming the enumerator it could not build |

## RX-166 — Single substitution with nothing to substitute is an error

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code substitutes once with neither a block nor a replacement |
| Then | an argument error is raised |
