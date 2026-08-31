# MatchData

What a successful match hands the guest, and how it is read.

## Includes

- `test/e2e/regexp/test_match_data.rb`
- `test/e2e/regexp/test_match_data_aref.rb`
- `test/e2e/regexp/test_match_data_bounds.rb`

### Why these scenarios

A match is a snapshot the guest reads several ways — as a list, by number, by name, and as offsets into the subject. Each reading is witnessed on its own because they are separate accessors over one state, and one of them drifting would not disturb the others.

The out-of-range readings are errors rather than absences, while a group that was in range and simply did not participate answers nothing. Telling those two apart is what lets guest code branch on an optional group without rescuing.

A match cannot be constructed. It exists because a pattern matched, which is what makes the offsets it carries mean anything about the subject it names.

## `RX-095` A match lists the whole match before its captures

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code converts a match to an Array |
| Then | the whole match comes first, then each capture |

## `RX-096` A named capture is reachable by its name as a Symbol

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code indexes a match by a capture name written as a Symbol |
| Then | it answers that capture |

## `RX-097` The same name written as a String reaches it too

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code indexes a match by a capture name written as a String |
| Then | it answers that capture |

## `RX-098` A match knows where it started

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's beginning |
| Then | it answers the byte offset where the match starts |

## `RX-099` A match knows where it ended

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's end |
| Then | it answers the byte offset just past the match |

## `RX-100` The two are readable as one pair

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's offset |
| Then | it answers the beginning and the end together |

## `RX-101` A match knows the text before it

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's preceding text |
| Then | it answers the substring before the match |

## `RX-102` A match knows the text after it

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's following text |
| Then | it answers the substring after the match |

## `RX-103` The captures alone are readable without the whole match

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's captures |
| Then | it answers each group and not the whole match |

## `RX-104` Named captures map each name to what it caught

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's named captures |
| Then | each name maps to its captured substring |

## `RX-105` A match lists the names it carries

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's names |
| Then | they come back in order |

## `RX-106` Named captures can be keyed by Symbol on request

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's named captures asking for symbolized names |
| Then | the keys are Symbols |

## `RX-107` A match's size counts itself as well as its groups

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's size |
| Then | it counts the whole match plus each group |

## `RX-108` A match arises from matching and nowhere else

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code tries to construct a match directly |
| Then | a no-method error is raised |

## `RX-109` Indexing with a start and a length answers that slice

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code indexes a match with a start and a length |
| Then | it answers that slice of the group list |

## `RX-110` Indexing with a range answers that slice

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code indexes a match with a Range |
| Then | it answers that slice of the group list |

## `RX-111` A negative index counts from the end of the list

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code indexes a match with a negative index |
| Then | it answers the group that far from the end |

## `RX-112` A name the pattern never declared is an error, not nothing

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code indexes a match by a capture name the pattern does not declare |
| Then | an index error is raised |

## `RX-113` A beginning past the group count is an error

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's beginning for an index past its group count |
| Then | an index error is raised |

## `RX-114` So is an end past the group count

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's end for an index past its group count |
| Then | an index error is raised |

## `RX-115` And an offset past it

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's offset for an index past its group count |
| Then | an index error is raised |

## `RX-116` A group's beginning is reachable by the group's name

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads a match's beginning for a capture name |
| Then | it answers that group's byte offset |

## `RX-117` A group that did not take part has no beginning

| Step | Statement |
| --- | --- |
| Given | a Sandbox over the regexp-capable Guest Binary |
| When | guest code reads the beginning of a valid group that did not participate |
| Then | it answers nothing |
