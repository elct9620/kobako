# Dir

What a script reaches through the default Guest Binary's `Dir`, beneath the paths a Mount places.

```
script ─ Dir ─> wasi-libc ─ preopen at the Mount's path ─> runtime ─> Filesystem
```

### Why these scenarios

`Dir` is the half of a Mount that `File` leaves out. It lists, makes, removes, and matches directories, and changes the working directory. It ships beside `File`, so the two share one rule. With no Mount it reaches nothing, and the `disable_file` flag removes it with `File`.

Each answer and each failure is CRuby's on Linux, so a script written for CRuby reads the same results here.

```
list a directory   -> its names, "." and ".." only from entries and read
make, remove       -> 0
match a pattern    -> CRuby's paths, in CRuby's order
change directory   -> 0, relative paths resolving from it
nothing mounted    -> NotImplementedError
disable_file flag  -> no Dir constant
```

The working directory is wasi-libc's, and it starts at `/` in every invocation. The guest reads no link's target, for security. So a directory reached through a link is named by the link's path, and `..` steps back by name.

What the guest cannot answer as CRuby does, it leaves out, so a script fails instead of reading another answer. An opened `Dir` holds no descriptor. The guest has no `Enumerator`, no `HOME`, and no String encoding.

```
fileno                    -> NoMethodError
a call given no block     -> ArgumentError, where CRuby answers an Enumerator
chdir naming no directory -> ArgumentError, as CRuby without HOME
chdir out of every Mount  -> Errno::ENOENT or EACCES, as WASI
the encoding: keyword     -> ArgumentError
```

A glob of `**/` with `base:` leaves out the `/` CRuby answers for the base itself, an answer naming no entry.

## Includes

- `test/e2e/test_dir.rb`
- `test/e2e/test_dir_opened.rb`
- `test/e2e/test_dir_glob.rb`
- `test/e2e/test_dir_working_directory.rb`

## `FL-050` A script lists a directory beneath a Mount

| Attribute | Value |
| --- | --- |
| relates | `FL-001`, `RT-075` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount over a directory holding two files |
| When | the script asks `Dir.children`, `Dir.entries`, and `Dir.each_child` of that directory |
| Then | each answers the two names, and `Dir.entries` adds `.` and `..` |

## `FL-051` A script makes and removes a directory beneath a read-write Mount

| Attribute | Value |
| --- | --- |
| relates | `FL-002` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount over a Tmpfs |
| When | the script makes directories with `Dir.mkdir`, then removes them with `Dir.rmdir`, `Dir.delete`, and `Dir.unlink` |
| Then | each call answers 0, and `Dir.exist?` follows each change |

## `FL-052` A script asks whether a path is a directory and whether it is empty

| Attribute | Value |
| --- | --- |
| refines | `FL-050` |
| relates | `FL-040` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount over a file, an empty directory, and a directory holding a file |
| When | the script asks `Dir.exist?` and `Dir.empty?` of each, and of a missing path |
| Then | only the directories exist, only the empty one is empty, and `Dir.empty?` of the missing path raises `Errno::ENOENT` |

## `FL-053` An opened directory yields its entries one at a time

| Attribute | Value |
| --- | --- |
| refines | `FL-050` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount over a directory holding two files |
| When | the script opens it with `Dir.open`, reads each entry, iterates it with `each`, and closes it |
| Then | `read` answers each entry then `nil`, `each` yields them, and a block given to `Dir.open` answers its value |

## `FL-054` A closed directory raises IOError

| Attribute | Value |
| --- | --- |
| refines | `FL-053` |
| relates | `FL-045` |

| Step | Statement |
| --- | --- |
| Given | a script holding a directory it opened and then closed |
| When | it reads the directory, and closes it again |
| Then | the read raises `IOError`, and the second close answers `nil` |

## `FL-055` A directory call a path cannot take raises CRuby's Errno

| Attribute | Value |
| --- | --- |
| refines | `FL-050`, `FL-051` |
| relates | `FL-005` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount over a file and a directory holding a file |
| When | the script lists a missing path and the file, makes the directory again, and removes the directory and the file |
| Then | they raise `Errno::ENOENT`, `Errno::ENOTDIR`, `Errno::EEXIST`, `Errno::ENOTEMPTY`, and `Errno::ENOTDIR` |

## `FL-056` Making a directory beneath a read-only Mount raises EROFS

| Attribute | Value |
| --- | --- |
| refines | `FL-051` |
| relates | `FL-006` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-only Mount |
| When | the script makes a directory beneath it |
| Then | the script raises `Errno::EROFS` |

## `FL-057` A directory call with nothing mounted is unimplemented

| Attribute | Value |
| --- | --- |
| relates | `FL-004`, `FL-042` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed no Mount |
| When | the script lists, opens, makes, removes, matches, changes into, or asks about a directory |
| Then | each call raises `NotImplementedError` |

## `FL-058` The disable_file flag removes Dir

| Attribute | Value |
| --- | --- |
| refines | `FL-008` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox that set the critical flag `disable_file` |
| When | the script asks whether `Dir` is defined |
| Then | it is not |

## `FL-071` A permission that is not a number raises TypeError

| Attribute | Value |
| --- | --- |
| refines | `FL-051` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a read-write Mount |
| When | the script makes a directory with a String permission, and another with `nil` |
| Then | each raises `TypeError`, and neither directory exists |

## `FL-072` An opened directory lists its children again and rewinds

| Attribute | Value |
| --- | --- |
| refines | `FL-053` |

| Step | Statement |
| --- | --- |
| Given | a script holding an opened directory with two files |
| When | it asks `children`, yields `each_child`, reads, then calls `rewind` and reads again |
| Then | each lists the two names, the first read answers `nil`, and the read after `rewind` answers `.` |

## `FL-073` A directory names its path open or closed

| Attribute | Value |
| --- | --- |
| refines | `FL-053` |
| relates | `FL-054` |

| Step | Statement |
| --- | --- |
| Given | a script holding a directory it opened |
| When | it asks `inspect` and `path`, closes it, and asks `path` again |
| Then | `inspect` names the path as `#<Dir:path>`, and each `path` answers it |

## `FL-092` A script finds the paths a pattern names beneath a Mount

| Attribute | Value |
| --- | --- |
| relates | `FL-050` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount over files and directories, some named with a leading dot |
| When | the script asks `Dir.glob` with `*`, `?`, a bracket, braces, `**/`, and a trailing separator, with and without `File::FNM_DOTMATCH` |
| Then | each answers the paths CRuby answers, in CRuby's order |

## `FL-093` A pattern's options choose where it reads and what it answers

| Attribute | Value |
| --- | --- |
| refines | `FL-092` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount over files and directories |
| When | the script asks `Dir.glob` with `base:`, `sort: false`, `File::FNM_NOESCAPE`, an Array, and a block, then `Dir[]` with several patterns |
| Then | each answers the paths CRuby answers, and the call given a block yields each and answers `nil` |

## `FL-094` A pattern walk follows a link only where a wildcard names it

| Attribute | Value |
| --- | --- |
| refines | `FL-092` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind whose directory holds a link to a directory |
| When | the script asks `Dir.glob` with `**/`, with `*/`, and with `*/**/` |
| Then | `**/` descends past the link, and a wildcard naming the link descends through it, as CRuby's do |

## `FL-095` A pattern a glob cannot take raises as CRuby's does

| Attribute | Value |
| --- | --- |
| refines | `FL-092` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount |
| When | the script asks `Dir.glob` of a pattern holding a NUL, of an Integer, and with `sort: nil` |
| Then | the NUL and `sort: nil` raise `ArgumentError`, and the Integer raises `TypeError` |

## `FL-096` A script changes its working directory beneath a Mount

| Attribute | Value |
| --- | --- |
| relates | `FL-050` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount over a directory holding a file and a directory |
| When | the script asks `Dir.pwd`, calls `Dir.chdir` into the directory, reads the file and lists by relative path, then calls `Dir.chdir("..")` |
| Then | `Dir.pwd` first answers `/`, each change answers `0`, each relative path resolves from the working directory, and `..` steps back |

## `FL-097` A block changes the working directory only while it runs

| Attribute | Value |
| --- | --- |
| refines | `FL-096` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount over a directory |
| When | the script calls `Dir.chdir` into it with a block that answers a value, then with one that raises |
| Then | each block receives the path and runs there, and the first call answers the block's value. The earlier directory returns after both |

## `FL-098` Each invocation starts at the root directory

| Attribute | Value |
| --- | --- |
| refines | `FL-096` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox whose earlier invocation changed into a directory beneath a Mount |
| When | a later invocation asks `Dir.pwd` |
| Then | it answers `/` |

## `FL-099` A change into what is no directory raises as CRuby's does

| Attribute | Value |
| --- | --- |
| refines | `FL-096` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount over a file |
| When | the script calls `Dir.chdir` to a missing path, to the file, and with no path |
| Then | they raise `Errno::ENOENT`, `Errno::ENOTDIR`, and `ArgumentError`, the working directory unchanged |

## `FL-100` A directory reached through a link is named by the link's path

| Attribute | Value |
| --- | --- |
| refines | `FL-096` |
| relates | `FS-048` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Bind whose directory holds a link to a directory |
| When | the script calls `Dir.chdir` through the link and asks `Dir.pwd` |
| Then | it answers the link's path, where CRuby on Linux names the directory it reaches |

## `FL-102` A pattern takes a name's characters and its own spelling as CRuby's does

| Attribute | Value |
| --- | --- |
| refines | `FL-092` |

| Step | Statement |
| --- | --- |
| Given | an invocation handed a Mount over files, one named with a character of several bytes |
| When | the script asks `Dir.glob` with `?` and a bracket, with an unclosed brace, and with a trailing backslash |
| Then | each answers the paths CRuby answers |
