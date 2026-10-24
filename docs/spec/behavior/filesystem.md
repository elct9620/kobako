# Filesystem

What each Filesystem kobako ships holds, and how long what a guest writes there lasts.

```
guest ─ system interface ─> runtime (descriptors, access, paths) ─> Filesystem
                                                                     these scenarios
```

### Why these scenarios

A Filesystem answers only for files and directories. The runtime in front of it owns descriptors, access, and path resolution. So these scenarios read each kind directly, without a guest. The guest's view through a Mount belongs to the runtime's own scenarios.

Two properties set the kinds apart: where the content lives, and how long it lasts. Each kind is witnessed on both. The Directory kind adds a third property, that nothing outside its directory is reachable.

```
kind       content lives       lasts
Memory     in memory           as long as the Host App's object
Temporary  in memory           one Invocation
Directory  in a host directory as long as the directory
Overlay    in its layers       as long as its writable layers
```

An Overlay stacks Layers, each read-only or writable on its own. The rules follow the union filesystems that allow that, aufs among them. Its markers follow the OCI image layer format, so a Host App reads them as it would a container layer.

```
the first layer added sits on top
read     the topmost layer holding the path answers, unless a whiteout hides it
change   a file in a writable layer changes in place
         a file in a read-only layer is copied up to the nearest writable layer
create   lands in the topmost writable layer, as a rename's target does
remove   leaves every writable layer it empties; a whiteout hides what any layer keeps
nothing writable  every change is refused as read-only
covered           a change a read-only layer above would hide is refused as read-only

whiteout          .wh.<name>      beside the hidden entry
opaque directory  .wh..wh..opq    inside a directory made again where one was removed
both live in the topmost writable layer, never show, and no name may claim .wh. in any case
```

## Includes

- `crates/kobako-runtime/src/filesystem/*.rs`

## `FS-001` A Memory serves the files it was seeded with

| Step | Statement |
| --- | --- |
| Given | a Memory seeded with a file |
| When | the file is opened and read |
| Then | it reads the seeded content |

## `FS-002` A Memory keeps what was written for as long as it lives

| Attribute | Value |
| --- | --- |
| relates | `FS-001` |

| Step | Statement |
| --- | --- |
| Given | a Memory a file was written to |
| When | the file is opened again from the same Memory |
| Then | it reads what was written |

## `FS-003` Every Invocation shares the one Memory

| Attribute | Value |
| --- | --- |
| relates | `FS-002` |

| Step | Statement |
| --- | --- |
| Given | a Memory |
| When | the next Invocation's Filesystem is taken from it |
| Then | that Invocation is handed the same Memory |

## `FS-004` A Temporary starts every Invocation empty

| Attribute | Value |
| --- | --- |
| relates | `FS-003` |

| Step | Statement |
| --- | --- |
| Given | a Temporary a file was written to |
| When | the next Invocation's Filesystem is taken from it |
| Then | that Filesystem holds no file |

## `FS-005` A Directory serves the files of its host directory

| Step | Statement |
| --- | --- |
| Given | a host directory holding a file |
| When | a Directory over it opens and reads that file |
| Then | it reads the host file's content |

## `FS-006` A Directory writes into its host directory

| Attribute | Value |
| --- | --- |
| relates | `FS-005` |

| Step | Statement |
| --- | --- |
| Given | a Directory over a host directory |
| When | a file is created and written through it |
| Then | the host directory holds that file with that content |

## `FS-007` A Directory reaches nothing outside its host directory

| Attribute | Value |
| --- | --- |
| relates | `FS-005` |

| Step | Statement |
| --- | --- |
| Given | a Directory over a host directory holding a link to a file outside it |
| When | the link's path is opened |
| Then | the open is refused |

## `FS-008` An Overlay reads through to a lower Layer

| Step | Statement |
| --- | --- |
| Given | an Overlay whose top Layer lacks a file a lower Layer holds |
| When | the file is read through the Overlay |
| Then | it reads the lower file's content |

## `FS-009` A write to a read-only Layer's file lands in the nearest writable Layer above it

| Attribute | Value |
| --- | --- |
| relates | `FS-008` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose writable Layer sits over a read-only Layer holding a file |
| When | the file is written through the Overlay |
| Then | the writable Layer holds what was written |

## `FS-010` An Overlay leaves a read-only Layer as it was

| Attribute | Value |
| --- | --- |
| relates | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose read-only Layer holds a file |
| When | the file is written and then removed through the Overlay |
| Then | the read-only Layer still holds the file as it was |

## `FS-011` A file removed through an Overlay is gone from its view

| Attribute | Value |
| --- | --- |
| relates | `FS-010` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose read-only Layer holds a file |
| When | the file is removed through the Overlay |
| Then | the Overlay finds no file at that path |

## `FS-012` An Overlay over a Temporary starts every Invocation from its lower Layers

| Attribute | Value |
| --- | --- |
| relates | `FS-004`, `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose writable Layer is a Temporary, with a lower file overwritten through it |
| When | the next Invocation's Filesystem is taken from it |
| Then | that Filesystem reads the lower file's content |

## `FS-013` An upper Layer hides the same path in a lower one

| Attribute | Value |
| --- | --- |
| relates | `FS-008` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose two Layers both hold a file at one path |
| When | the file is read through the Overlay |
| Then | it reads the upper Layer's content |

## `FS-014` A write to a writable Layer's file changes it in place

| Attribute | Value |
| --- | --- |
| refines | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay of two writable Layers, the lower one holding a file |
| When | the file is written through the Overlay |
| Then | the lower Layer holds what was written |

## `FS-015` A new file lands in the topmost writable Layer

| Attribute | Value |
| --- | --- |
| relates | `FS-014` |

| Step | Statement |
| --- | --- |
| Given | an Overlay of a read-only Layer over two writable Layers |
| When | a new file is created through the Overlay |
| Then | the upper writable Layer holds it |

## `FS-016` A removal leaves a whiteout in the topmost writable Layer

| Attribute | Value |
| --- | --- |
| refines | `FS-011` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose writable Layer sits over a read-only Layer holding a file |
| When | the file is removed through the Overlay |
| Then | the writable Layer holds `.wh.` and the file's name beside where it was |

## `FS-017` A whiteout lasts as long as its writable Layer

| Attribute | Value |
| --- | --- |
| relates | `FS-002`, `FS-016` |

| Step | Statement |
| --- | --- |
| Given | a lower file removed through an Overlay whose writable Layer is a Memory |
| When | a new Overlay is built over the same Layers |
| Then | the new Overlay finds no file at that path |

## `FS-018` A directory made again where one was removed shows nothing from below

| Attribute | Value |
| --- | --- |
| relates | `FS-016` |

| Step | Statement |
| --- | --- |
| Given | a read-only Layer's directory removed through an Overlay with every file in it |
| When | the directory is created again through the Overlay |
| Then | listing it finds no entry |

## `FS-019` An Overlay lists none of its markers

| Attribute | Value |
| --- | --- |
| relates | `FS-016` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose writable Layer holds a whiteout |
| When | the whiteout's directory is listed through the Overlay |
| Then | no entry starting with `.wh.` is listed |

## `FS-020` An Overlay refuses a name reserved for its markers

| Attribute | Value |
| --- | --- |
| relates | `FS-019` |

| Step | Statement |
| --- | --- |
| Given | an Overlay with a writable Layer |
| When | a file named `.wh.note` is created through the Overlay |
| Then | the creation is refused |

## `FS-021` An Overlay with no writable Layer refuses every change as read-only

| Attribute | Value |
| --- | --- |
| relates | `FS-015` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose Layers are all read-only |
| When | a file is written through the Overlay |
| Then | the write is refused as read-only |

## `FS-022` A write past a Memory's limit is refused as out of space

| Attribute | Value |
| --- | --- |
| relates | `FS-002` |

| Step | Statement |
| --- | --- |
| Given | a Memory given a limit its file already nears |
| When | a write would grow its files past that limit |
| Then | the write is refused as out of space, and the file holds what it held |

## `FS-023` A Memory given no limit of its own holds 64 MiB

| Attribute | Value |
| --- | --- |
| refines | `FS-022` |

| Step | Statement |
| --- | --- |
| Given | a Memory created without a limit |
| When | a file grows to 64 MiB, and then one byte past it |
| Then | the first growth lands and the second is refused as out of space |

## `FS-024` A Memory whose limit is set to none is not bounded by it

| Attribute | Value |
| --- | --- |
| refines | `FS-022` |

| Step | Statement |
| --- | --- |
| Given | a Memory whose limit is set to none |
| When | a file grows one byte past 64 MiB |
| Then | the growth lands |

## `FS-025` A removed file gives its space back

| Attribute | Value |
| --- | --- |
| relates | `FS-022` |

| Step | Statement |
| --- | --- |
| Given | a Memory whose one file fills its limit |
| When | the file is removed and another file is written as large |
| Then | the second write lands |

## `FS-026` A growth past what can be allocated is refused as out of space

| Attribute | Value |
| --- | --- |
| refines | `FS-024` |

| Step | Statement |
| --- | --- |
| Given | a Memory whose limit is set to none |
| When | a file grows past what any allocation can hold |
| Then | the growth is refused as out of space, and the file holds what it held |

## `FS-027` A Temporary keeps its limit in every Invocation

| Attribute | Value |
| --- | --- |
| relates | `FS-004`, `FS-022` |

| Step | Statement |
| --- | --- |
| Given | a Temporary given a limit |
| When | the next Invocation's Filesystem is written past that limit |
| Then | the write is refused as out of space |

## `FS-028` A Directory refuses to remove a directory as a file

| Attribute | Value |
| --- | --- |
| relates | `FS-006` |

| Step | Statement |
| --- | --- |
| Given | a Directory whose host directory holds a directory |
| When | that directory is removed as a file |
| Then | the removal answers that it is a directory, on every host |

## `FS-029` A Directory on Windows refuses a name holding a backslash

| Attribute | Value |
| --- | --- |
| relates | `FS-007`, `RT-080` |

| Step | Statement |
| --- | --- |
| Given | a Directory on a Windows host |
| When | a path holding a backslash is opened there |
| Then | the open is refused as invalid, and the host is not asked |

## `FS-030` A Memory holds at most its inodes

| Attribute | Value |
| --- | --- |
| relates | `FS-022` |

| Step | Statement |
| --- | --- |
| Given | a Memory given a count of inodes, its root counted among them |
| When | a file or a directory is created past that count |
| Then | the creation is refused as out of space |

## `FS-031` A Memory given only a limit holds an inode for each 4 KiB of it

| Attribute | Value |
| --- | --- |
| refines | `FS-030` |

| Step | Statement |
| --- | --- |
| Given | a Memory given a limit of 8 KiB and no inodes of its own |
| When | three files are created |
| Then | the first two land and the third is refused as out of space |

## `FS-032` A removed file gives its inode back once no handle holds it

| Attribute | Value |
| --- | --- |
| relates | `FS-025`, `FS-030` |

| Step | Statement |
| --- | --- |
| Given | a Memory whose inodes are all taken, one by a file the guest holds open |
| When | the file is removed, and a file is created before and after its handle closes |
| Then | the creation before is refused as out of space and the one after lands |

## `FS-033` A Temporary keeps its inodes in every Invocation

| Attribute | Value |
| --- | --- |
| relates | `FS-027`, `FS-030` |

| Step | Statement |
| --- | --- |
| Given | a Temporary given a count of inodes |
| When | the next Invocation's Filesystem is given an entry past that count |
| Then | the creation is refused as out of space |

## `FS-034` A removed directory gives its inode back

| Attribute | Value |
| --- | --- |
| relates | `FS-032` |

| Step | Statement |
| --- | --- |
| Given | a Memory whose inodes are all taken, some by directories |
| When | a directory is removed, or replaced by renaming another onto it |
| Then | a directory created afterwards lands |

## `FS-035` A Memory moves only what a rename allows

| Attribute | Value |
| --- | --- |
| relates | `RT-094` |

| Step | Statement |
| --- | --- |
| Given | a Memory holding a directory and a file |
| When | the directory is renamed as a file only, and the file as a directory only |
| Then | each is refused, as crossing devices and as no directory, and neither moves |

## `FS-036` A Memory appends at its file's end as it stands

| Attribute | Value |
| --- | --- |
| relates | `FS-002` |

| Step | Statement |
| --- | --- |
| Given | a Memory file open through two handles |
| When | each handle appends in turn |
| Then | each append lands after the one before, and answers where it landed |

## `FS-037` A change through an Overlay waits for the one in progress

| Attribute | Value |
| --- | --- |
| relates | `FS-011` |

| Step | Statement |
| --- | --- |
| Given | an Overlay removing an empty directory |
| When | another thread creates a file in it before the removal returns |
| Then | the creation waits, and the removal finds the directory empty |

## `FS-038` A call through an Overlay from inside one answers a deadlock

| Attribute | Value |
| --- | --- |
| refines | `FS-037` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose Layer calls back into that Overlay on the same thread |
| When | the Layer's call back is made |
| Then | it answers a deadlock rather than waiting |

## `FS-039` A contended Overlay waits through its Blocking

| Attribute | Value |
| --- | --- |
| refines | `FS-037` |

| Step | Statement |
| --- | --- |
| Given | an Overlay given a Blocking, its lock held by another thread |
| When | a call is made through the Overlay |
| Then | it waits through the Blocking once, then answers |

## `FS-040` A removal through an Overlay that cannot leave its whiteout removes nothing

| Attribute | Value |
| --- | --- |
| relates | `FS-016`, `FS-030` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose full writable Layer holds a copy of a lower file |
| When | the file is removed |
| Then | the removal is refused as out of space, and the copy still reads |

## `FS-041` A directory an Overlay cannot mark opaque is not made

| Attribute | Value |
| --- | --- |
| relates | `FS-018`, `FS-030` |

| Step | Statement |
| --- | --- |
| Given | an Overlay hiding a lower directory, its writable Layer with room for one entry |
| When | the directory is made again |
| Then | the creation is refused as out of space, and the directory stays hidden |

## `FS-042` A rename through an Overlay that cannot leave its whiteout moves nothing

| Attribute | Value |
| --- | --- |
| relates | `FS-016`, `FS-030` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose writable Layer has room only to copy a lower file up |
| When | the file is renamed |
| Then | the rename is refused as out of space, and the file stays where it was |

## `FS-043` A directory a lower Layer holds does not move through an Overlay

| Attribute | Value |
| --- | --- |
| relates | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose read-only Layer holds a directory |
| When | the directory is renamed |
| Then | the rename is refused as crossing devices, and nothing moves |

## `FS-044` A rename onto what an Overlay shows is checked against the view

| Attribute | Value |
| --- | --- |
| relates | `FS-043` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose read-only Layer alone shows a directory holding a file |
| When | a file, then an empty directory, is renamed onto that directory |
| Then | each is refused as rename(2) refuses it, and the directory stays as it was |

## `FS-045` A directory moved over a lower one is marked opaque

| Attribute | Value |
| --- | --- |
| refines | `FS-044` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose read-only Layer holds an empty directory |
| When | a directory the writable Layer holds is renamed onto it |
| Then | the moved directory is marked opaque in the writable Layer |

## `FS-046` A copy up that fails part way leaves nothing behind

| Attribute | Value |
| --- | --- |
| relates | `FS-009`, `FS-022` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose writable Layer has less room than a lower file holds |
| When | the lower file is written, copying it up |
| Then | the write is refused as out of space, and the lower file still reads |

## `FS-047` A Directory opens no link an open asks not to follow

| Attribute | Value |
| --- | --- |
| relates | `FS-007` |

| Step | Statement |
| --- | --- |
| Given | a Directory whose host directory holds a link to a file beside it |
| When | the link is opened asking to follow none, and then asking to follow it |
| Then | the first open is refused as a link and the second reaches the file |

## `FS-048` A Directory reports a link a lookup does not follow as a link

| Attribute | Value |
| --- | --- |
| relates | `FS-047` |

| Step | Statement |
| --- | --- |
| Given | a Directory whose host directory holds a link to a file beside it |
| When | the link is looked up following none, and its directory is listed |
| Then | both report a link with no size, and a lookup that follows reports the file |

## `FS-049` An Overlay reports a link its Layer holds as a link

| Attribute | Value |
| --- | --- |
| refines | `FS-048` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose read-only Layer is a Directory holding a link to a file and one to nothing |
| When | each link is looked up through the Overlay, following none |
| Then | each is reported as a link |

## `FS-050` An Overlay removes a link its read-only Layer holds as itself

| Attribute | Value |
| --- | --- |
| refines | `FS-010` |
| relates | `FS-049` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose read-only Directory Layer holds a link to a directory and one to nothing |
| When | each link is removed as a directory and then as a file |
| Then | the first is refused as no directory, the rest leave the view, and the Layer stays as it was |

## `FS-051` An Overlay refuses a change through a link its read-only Layer holds

| Attribute | Value |
| --- | --- |
| refines | `FS-009` |
| relates | `FS-049` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose read-only Directory Layer holds links to a file, a directory, and nothing |
| When | the guest writes, creates, or renames through one of those links |
| Then | each change is refused as crossing devices, and the target stays as it was |

## `FS-052` An Overlay finds a dangling link where a new entry would go

| Attribute | Value |
| --- | --- |
| relates | `FS-049` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose read-only Directory Layer holds a link to nothing |
| When | a file is created there exclusively, or a directory is made there |
| Then | each is refused as already existing |

## `FS-053` A link a Layer holds hides what the Layers below hold there

| Attribute | Value |
| --- | --- |
| refines | `FS-008` |
| relates | `FS-049` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper Directory Layer holds a link to nothing where a lower Layer holds a file |
| When | the path is opened to read through the Overlay |
| Then | it is not found |

## `FS-054` A copy up through a Layer writing in parts copies the whole file

| Attribute | Value |
| --- | --- |
| refines | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose writable Layer writes a few bytes per call, over a lower file |
| When | the lower file is opened to write, copying it up |
| Then | the copy in the writable Layer holds the whole lower file |

## `FS-055` A copy up whose close a Layer refuses leaves nothing behind

| Attribute | Value |
| --- | --- |
| refines | `FS-009` |
| relates | `FS-046` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose writable Layer refuses the close of a file it wrote |
| When | the lower file is written, copying it up |
| Then | the write is refused, and the lower file still reads |

## `FS-056` An Overlay refuses its reserved name in any case

| Attribute | Value |
| --- | --- |
| refines | `FS-020` |

| Step | Statement |
| --- | --- |
| Given | an Overlay with a writable Layer |
| When | a file named `.WH.note` is created through the Overlay |
| Then | the creation is refused |

## `FS-057` A link a Layer holds hides what the Layers below hold beneath it

| Attribute | Value |
| --- | --- |
| refines | `FS-053` |
| relates | `FS-013` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose writable Directory Layer holds `dirlink` as a link to a directory, over a Layer holding `dirlink/x` |
| When | `dirlink` is listed and `dirlink/x` is looked up through the Overlay |
| Then | the listing shows only where the link points, and `dirlink/x` is not found |

## `FS-058` Removing a file over a lower directory leaves the directory hidden

| Attribute | Value |
| --- | --- |
| refines | `FS-011` |
| relates | `FS-013`, `FS-016` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose higher writable Layer holds file `f` over a lower writable Layer's directory `f` |
| When | `f` is removed through the Overlay |
| Then | `f` is gone from the view, and the lower directory is as it was |

## `FS-059` A directory moves onto one whose writable copy holds only markers

| Attribute | Value |
| --- | --- |
| relates | `FS-016`, `FS-018` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose directory `dir` shows empty, its writable copy holding only the whiteout of a removed lower file |
| When | another directory is renamed onto `dir` through the Overlay |
| Then | the rename succeeds, and `dir` shows only the moved directory's entries |

## `FS-060` A directory made again moves as the writable Layer's own

| Attribute | Value |
| --- | --- |
| refines | `FS-018` |

| Step | Statement |
| --- | --- |
| Given | an Overlay where a read-only Layer's directory was removed and made again |
| When | the directory is renamed through the Overlay |
| Then | it moves, and nothing of the read-only Layer's directory shows at either path |

## `FS-061` An Overlay refuses a file moved beneath itself as no directory

| Attribute | Value |
| --- | --- |
| relates | `RT-105` |

| Step | Statement |
| --- | --- |
| Given | an Overlay showing the file `dir/file` |
| When | `dir/file` is renamed to `dir/file/x` through the Overlay |
| Then | the rename is refused as no directory |

## `FS-062` A Memory seeded with a list it cannot hold panics

| Attribute | Value |
| --- | --- |
| refines | `FS-001` |

| Step | Statement |
| --- | --- |
| Given | a list naming the root, a file twice, or a file where another path needs a directory |
| When | a Memory is seeded with it |
| Then | the seeding panics |

## `FS-063` A refused change leaves what a whiteout hid hidden

| Attribute | Value |
| --- | --- |
| refines | `FS-016` |
| relates | `FS-059` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose writable Layer holds only a whiteout in `dir`, and refuses to move or remove a directory |
| When | a directory is renamed onto `dir`, and `dir` is removed, through the Overlay |
| Then | both are refused, and the whited-out file stays hidden |

## `FS-064` A Directory opens a file already there without writing to create it

| Attribute | Value |
| --- | --- |
| relates | `RT-103`, `FS-006` |

| Step | Statement |
| --- | --- |
| Given | a Directory whose host directory holds a file only reading is allowed on |
| When | the file is opened asking to create it but not to write, and so is a missing one |
| Then | the file opens, and the missing one is created |

## `FS-065` A Directory answers a host failure as the error naming it

| Attribute | Value |
| --- | --- |
| relates | `RF-015`, `FS-005` |

| Step | Statement |
| --- | --- |
| Given | a Directory over a host directory |
| When | a file is created under a name longer than the host allows |
| Then | the creation fails as a name too long, not as an unnamed failure |

## `FS-066` A Directory refuses to list a host name that is not UTF-8

| Attribute | Value |
| --- | --- |
| relates | `FS-065`, `FL-018` |

| Step | Statement |
| --- | --- |
| Given | a Directory on a Linux host whose host directory holds a name that is not UTF-8 |
| When | the directory is listed |
| Then | the listing fails as an illegal byte sequence, not as an unnamed failure |

## `FS-067` A Memory tried with a list it cannot hold answers why

| Attribute | Value |
| --- | --- |
| relates | `FS-062` |

| Step | Statement |
| --- | --- |
| Given | a list naming the root, a file twice, or a file where another path needs a directory |
| When | a Memory is tried with it |
| Then | the trial answers the error naming what the list holds wrong |

## `FS-068` A Directory answers a host file too large as too large

| Attribute | Value |
| --- | --- |
| refines | `FS-065` |

| Step | Statement |
| --- | --- |
| Given | a Directory over a host directory |
| When | the host refuses a write because the file would grow too large |
| Then | the write fails as a file too large, not as an unnamed failure |

## `FS-069` A Directory appends at its host file's end in one step

| Attribute | Value |
| --- | --- |
| relates | `FS-036`, `RT-095` |

| Step | Statement |
| --- | --- |
| Given | a Directory file open through two handles |
| When | both handles append at the same time |
| Then | every append keeps its bytes, and none lands over another |

## `FS-070` A Memory gives each directory it makes an identity of its own

| Attribute | Value |
| --- | --- |
| relates | `RT-117` |

| Step | Statement |
| --- | --- |
| Given | a Memory holding a directory |
| When | the directory moves, and another is made at its old path |
| Then | the moved directory keeps its identity, and the new one has another |

## `FS-071` A Directory reports the identity its host gives a path

| Attribute | Value |
| --- | --- |
| relates | `RT-117` |

| Step | Statement |
| --- | --- |
| Given | a Directory over a host directory |
| When | a path beneath it is asked for its metadata |
| Then | the metadata carries the device and inode numbers the host reports |

## `FS-072` An Overlay keeps a directory's identity when a write copies it up

| Attribute | Value |
| --- | --- |
| relates | `RT-117` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose directory stands only in a read-only Layer |
| When | a file is made inside it, copying the directory up |
| Then | the directory keeps the identity it had before |

## `FS-073` An Overlay names a directory alike whether a link at its end is followed

| Attribute | Value |
| --- | --- |
| refines | `FS-072` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose directory stands in two Layers |
| When | the directory is looked up following a link at its end, and without |
| Then | both lookups report the same identity |

## `FS-074` An Overlay removes a directory a lower writable Layer still fills behind a whiteout

| Attribute | Value |
| --- | --- |
| refines | `FS-016` |
| relates | `FS-063` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper Layer hides a file a lower writable Layer holds in `dir` |
| When | `dir` is removed through the Overlay |
| Then | the view no longer holds `dir`, and the lower Layer still holds the file |

## `FS-075` A rename through an Overlay that cannot mark its directory opaque moves nothing

| Attribute | Value |
| --- | --- |
| refines | `FS-042` |
| relates | `FS-041` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose full writable Layer holds a directory, over an empty lower one |
| When | the first directory is renamed onto the second |
| Then | the rename is refused as out of space, and nothing moves |

## `FS-076` An Overlay refuses a rename a read-only Layer above would cover

| Attribute | Value |
| --- | --- |
| refines | `FS-015` |
| relates | `FS-010` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose writable Layer sits beneath a read-only Layer holding `to` |
| When | a lower file is renamed onto `to` |
| Then | the rename is refused as read-only, and both names read as before |

## `FS-077` An Overlay makes nothing a read-only Layer above would hide

| Attribute | Value |
| --- | --- |
| refines | `FS-015` |
| relates | `FS-076` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose writable Layer sits beneath a read-only Layer holding whiteouts |
| When | a file and a directory are made under the whited-out names |
| Then | both are refused as read-only, and the writable Layer holds neither |

## `FS-078` An Overlay shows a directory over a lower Layer's file on its path

| Attribute | Value |
| --- | --- |
| relates | `FS-057` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper Layer holds `dir/sub`, over a Layer holding a file `dir` |
| When | `dir/sub` is looked up through the Overlay |
| Then | it reads as a directory, as its files open |

## `FS-079` An Overlay fails a lookup when a Layer cannot read its markers

| Attribute | Value |
| --- | --- |
| relates | `FS-063` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper Layer whites out a lower file, yet reads no marker |
| When | the file is looked up through the Overlay |
| Then | the lookup fails, and the file never shows |

## `FS-080` An Overlay rename answers not found for a file its Layer loses

| Attribute | Value |
| --- | --- |
| relates | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay over a Layer whose file is gone after its first lookup |
| When | the file is renamed through the Overlay |
| Then | the rename answers not found |

## `FS-081` A Directory spells a path it matches exactly as given

| Step | Statement |
| --- | --- |
| Given | a Directory over a host directory holding `dir/file` |
| When | `dir/file`, and a path whose tail is missing, are asked for their stored spelling |
| Then | each comes back as given |

## `FS-082` A Directory spells a path as a folding host stores it

| Attribute | Value |
| --- | --- |
| refines | `FS-081` |

| Step | Statement |
| --- | --- |
| Given | a Directory over a macOS host directory folding case and Unicode form |
| When | a path is asked for its stored spelling in another case and form |
| Then | each name it holds comes back as the host stores it |

## `FS-083` An Overlay spells a path as the Layer showing it stores it

| Attribute | Value |
| --- | --- |
| refines | `FS-081` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer folds case and stores names in lower case |
| When | a path it shows, and a new name beneath it, are asked in upper case |
| Then | the names it holds come back in lower case, and the new name as given |

## `FS-084` A Directory spells a path as a folding Linux filesystem lists it

| Attribute | Value |
| --- | --- |
| refines | `FS-081` |
| unverifiable | a Linux filesystem that folds names, such as vfat, needs root to mount |

| Step | Statement |
| --- | --- |
| Given | a Directory over a Linux host directory whose filesystem folds names |
| When | a path is asked for its stored spelling in another case |
| Then | each name it holds comes back as the directory's listing names it |

## `FS-085` A Filesystem without a device of its own takes one no other names

| Attribute | Value |
| --- | --- |
| relates | `FS-070`, `FS-071` |

| Step | Statement |
| --- | --- |
| Given | a Memory, and two device numbers taken for Filesystems without one |
| When | their device numbers are compared, with each other and with a host's |
| Then | each differs from the others, and none is one a host reports |

## `FS-086` A Directory holding a directory keeps its inode from the next one made

| Attribute | Value |
| --- | --- |
| relates | `FS-071` |

| Step | Statement |
| --- | --- |
| Given | a Directory over a Linux host reusing inode numbers, holding a directory |
| When | the directory is removed and another made at its path |
| Then | the new directory takes another inode number |

## `FS-087` A Memory tried with a path through `..` answers why

| Attribute | Value |
| --- | --- |
| refines | `FS-067` |

| Step | Statement |
| --- | --- |
| Given | a list naming a path through `..` |
| When | a Memory is tried with it |
| Then | the trial answers the error naming that path |
