# Dir

What a script reaches through the default Guest Binary's `Dir`, beneath the paths a Mount places.

```
script ─ Dir ─> wasi-libc ─ preopen at the Mount's path ─> runtime ─> Filesystem
```

### Why these scenarios

`Dir` is the half of a Mount that `File` leaves out: it lists, makes, and removes directories. It ships beside `File`, so the two share one rule. With no Mount it reaches nothing, and the `disable_file` flag removes it with `File`.

Each answer and each failure is CRuby's on Linux, so a script written for CRuby reads the same results here.

```
list a directory   -> its names, "." and ".." only from entries and read
make, remove       -> 0
nothing mounted    -> NotImplementedError
disable_file flag  -> no Dir constant
```

What the guest cannot answer as CRuby does, it leaves out, so a script fails instead of reading another answer. An opened `Dir` holds no descriptor. The guest has no working directory, no `Enumerator`, and no String encoding.

```
glob, [], pwd, chdir, fileno -> NoMethodError
a call given no block        -> ArgumentError, where CRuby answers an Enumerator
the encoding: keyword        -> ArgumentError
```

## Includes

- `test/e2e/test_dir.rb`
- `test/e2e/test_dir_opened.rb`

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
| When | the script lists, opens, makes, removes, or asks about a directory |
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
