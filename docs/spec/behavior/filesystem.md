# Filesystem

What each Filesystem kobako ships holds, and how long what a guest writes there lasts.

```
guest ─ system interface ─> runtime (descriptors, access, paths) ─> Filesystem
                                                                     these scenarios
```

### Why these scenarios

A Filesystem answers only for files and directories. The runtime in front of it owns descriptors, access, and path resolution. So these scenarios read each kind directly, without a guest. The guest's view through a Mount belongs to the runtime's own scenarios.

Two properties set the kinds apart: where the content lives, and how long it lasts. Each kind is witnessed on both. The Bind kind adds a third property, that nothing outside its directory is reachable.

```
kind       content lives       lasts
Tmpfs     in memory           as long as the Host App's object
Bind  in a host directory as long as the directory
Overlay    in its layers       as long as its upper Tmpfs
```

An Overlay follows Linux overlayfs, with the upper restricted as gVisor restricts it. Its lower Layers are read-only, and its upper Layer is a Tmpfs, so the host stays as it was. A Host App that wants a change on the host mounts a Bind beside it.

```
lower Layers, topmost first; an upper Tmpfs on top of them, or none
read     the topmost layer holding the path answers, unless a whiteout hides it
change   a file in the upper changes in place; a lower file is copied up first
create   lands in the upper, as a rename's target does
remove   leaves the upper; a whiteout hides what a lower layer keeps
no upper every change is refused as read-only, and two lowers are needed

upper Tmpfs   upper/ holds the changes; work/ builds each before it lands
whiteout      a device 0:0 at the hidden name, as Linux's
opaque        trusted.overlay.opaque on a directory made again where one was removed
both live in the upper and never show, so no name is reserved
```

## Includes

- `crates/kobako-runtime/src/filesystem/*.rs`
- `crates/kobako-runtime/src/filesystem/bind/lookup.rs`

## `FS-001` A Tmpfs serves the files it was seeded with

| Step | Statement |
| --- | --- |
| Given | a Tmpfs seeded with a file |
| When | the file is opened and read |
| Then | it reads the seeded content |

## `FS-002` A Tmpfs keeps what was written for as long as it lives

| Attribute | Value |
| --- | --- |
| relates | `FS-001` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs a file was written to |
| When | the file is opened again from the same Tmpfs |
| Then | it reads what was written |

## `FS-005` A Bind serves the files of its host directory

| Step | Statement |
| --- | --- |
| Given | a host directory holding a file |
| When | a Bind over it opens and reads that file |
| Then | it reads the host file's content |

## `FS-006` A Bind writes into its host directory

| Attribute | Value |
| --- | --- |
| relates | `FS-005` |

| Step | Statement |
| --- | --- |
| Given | a Bind over a host directory |
| When | a file is created and written through it |
| Then | the host directory holds that file with that content |

## `FS-007` A Bind reaches nothing outside its host directory

| Attribute | Value |
| --- | --- |
| relates | `FS-005` |

| Step | Statement |
| --- | --- |
| Given | a Bind over a host directory holding a link to a file outside it |
| When | the link's path is opened |
| Then | the open is refused |

## `FS-008` An Overlay reads through to a lower Layer

| Step | Statement |
| --- | --- |
| Given | an Overlay whose top Layer lacks a file a lower Layer holds |
| When | the file is read through the Overlay |
| Then | it reads the lower file's content |

## `FS-009` A write to a lower Layer's file lands in the upper

| Attribute | Value |
| --- | --- |
| relates | `FS-008` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer holds a file |
| When | the file is written through the Overlay |
| Then | the upper holds what was written |

## `FS-010` An Overlay leaves a lower Layer as it was

| Attribute | Value |
| --- | --- |
| relates | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer holds a file |
| When | the file is written and then removed through the Overlay |
| Then | the lower Layer still holds the file as it was |

## `FS-011` A file removed through an Overlay is gone from its view

| Attribute | Value |
| --- | --- |
| relates | `FS-010` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer holds a file |
| When | the file is removed through the Overlay |
| Then | the Overlay finds no file at that path |

## `FS-013` An upper Layer hides the same path in a lower one

| Attribute | Value |
| --- | --- |
| relates | `FS-008` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose two Layers both hold a file at one path |
| When | the file is read through the Overlay |
| Then | it reads the upper Layer's content |

## `FS-014` A write to the upper's file changes it in place

| Attribute | Value |
| --- | --- |
| refines | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper holds a file |
| When | the file is written through the Overlay |
| Then | the upper's file holds what was written |

## `FS-015` A new file lands in the upper

| Attribute | Value |
| --- | --- |
| relates | `FS-014` |

| Step | Statement |
| --- | --- |
| Given | an Overlay over a lower Layer |
| When | a new file is created through the Overlay |
| Then | the upper holds it, and the lower Layer does not |

## `FS-016` A removal leaves a whiteout in the upper

| Attribute | Value |
| --- | --- |
| refines | `FS-011` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer holds a file |
| When | the file is removed through the Overlay |
| Then | the upper holds a whiteout at the file's name |

## `FS-017` A whiteout lasts as long as the upper

| Attribute | Value |
| --- | --- |
| relates | `FS-002`, `FS-016` |

| Step | Statement |
| --- | --- |
| Given | a lower file removed through an Overlay |
| When | a new Overlay is built over the same Layers |
| Then | the new Overlay finds no file at that path |

## `FS-018` A directory made again where one was removed shows nothing from below

| Attribute | Value |
| --- | --- |
| relates | `FS-016` |

| Step | Statement |
| --- | --- |
| Given | a lower Layer's directory removed through an Overlay with every file in it |
| When | the directory is created again through the Overlay |
| Then | listing it finds no entry |

## `FS-019` An Overlay lists none of its whiteouts

| Attribute | Value |
| --- | --- |
| relates | `FS-016` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper holds a whiteout |
| When | the whiteout's directory is listed through the Overlay |
| Then | the whited-out name is not listed |

## `FS-020` A name starting with `.wh.` is an ordinary name

| Attribute | Value |
| --- | --- |
| relates | `FS-019` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer holds `dir/file` |
| When | a file named `dir/.wh.file` is created through the Overlay |
| Then | it is made, and `dir/file` still reads |

## `FS-021` An Overlay with no upper refuses every change as read-only

| Attribute | Value |
| --- | --- |
| relates | `FS-015` |

| Step | Statement |
| --- | --- |
| Given | an Overlay of two lower Layers and no upper |
| When | a file is written through the Overlay |
| Then | the write is refused as read-only |

## `FS-022` A write past a Tmpfs's size is refused as out of space

| Attribute | Value |
| --- | --- |
| relates | `FS-002` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs given a size its file already nears |
| When | a write would grow its files past that size |
| Then | the write is refused as out of space, and the file holds what it held |

## `FS-023` A Tmpfs given no size of its own holds 64 MiB

| Attribute | Value |
| --- | --- |
| refines | `FS-022` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs created without a size |
| When | a file grows to 64 MiB, and then one byte past it |
| Then | the first growth lands and the second is refused as out of space |

## `FS-024` A Tmpfs whose size is set to none is not bounded by it

| Attribute | Value |
| --- | --- |
| refines | `FS-022` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs whose size is set to none |
| When | a file grows one byte past 64 MiB |
| Then | the growth lands |

## `FS-025` A removed file gives its space back

| Attribute | Value |
| --- | --- |
| relates | `FS-022` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs whose one file fills its size |
| When | the file is removed and another file is written as large |
| Then | the second write lands |

## `FS-026` A growth past what can be allocated is refused as out of space

| Attribute | Value |
| --- | --- |
| refines | `FS-024` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs whose size is set to none |
| When | a file grows past what any allocation can hold |
| Then | the growth is refused as out of space, and the file holds what it held |

## `FS-028` A Bind refuses to remove a directory as a file

| Attribute | Value |
| --- | --- |
| relates | `FS-006` |

| Step | Statement |
| --- | --- |
| Given | a Bind whose host directory holds a directory |
| When | that directory is removed as a file |
| Then | the removal answers that it is a directory, on every host |

## `FS-029` A Bind on Windows refuses a name holding a backslash

| Attribute | Value |
| --- | --- |
| relates | `FS-007`, `RT-080` |

| Step | Statement |
| --- | --- |
| Given | a Bind on a Windows host |
| When | a path holding a backslash is opened there |
| Then | the open is refused as invalid, and the host is not asked |

## `FS-030` A Tmpfs holds at most its inodes

| Attribute | Value |
| --- | --- |
| relates | `FS-022` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs given a count of inodes, its root counted among them |
| When | a file or a directory is created past that count |
| Then | the creation is refused as out of space |

## `FS-031` A Tmpfs given only a size holds an inode for each 4 KiB of it

| Attribute | Value |
| --- | --- |
| refines | `FS-030` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs given a size of 8 KiB and no inodes of its own |
| When | three files are created |
| Then | the first two land and the third is refused as out of space |

## `FS-032` A removed file gives its inode back once no handle holds it

| Attribute | Value |
| --- | --- |
| relates | `FS-025`, `FS-030` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs whose inodes are all taken, one by a file the guest holds open |
| When | the file is removed, and a file is created before and after its handle closes |
| Then | the creation before is refused as out of space and the one after lands |

## `FS-034` A removed directory gives its inode back

| Attribute | Value |
| --- | --- |
| relates | `FS-032` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs whose inodes are all taken, some by directories |
| When | a directory is removed, or replaced by renaming another onto it |
| Then | a directory created afterwards lands |

## `FS-036` A Tmpfs appends at its file's end as it stands

| Attribute | Value |
| --- | --- |
| relates | `FS-002` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs file open through two handles |
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

## `FS-038` A change through an Overlay from inside one of its reads answers a deadlock

| Attribute | Value |
| --- | --- |
| refines | `FS-037` |
| relates | `RT-166` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose Layer, asked during a read, reads and then changes that Overlay on the same thread |
| When | the Layer's calls back are made |
| Then | the read answers, and the change answers a deadlock rather than waiting |

## `FS-039` A contended copy up waits through the Overlay's Blocking

| Attribute | Value |
| --- | --- |
| refines | `FS-037` |

| Step | Statement |
| --- | --- |
| Given | an Overlay given a Blocking, one thread copying a lower file up |
| When | another thread writes the same file |
| Then | it waits through the Blocking, then lands |

## `FS-040` A removal through an Overlay that cannot leave its whiteout removes nothing

| Attribute | Value |
| --- | --- |
| relates | `FS-016`, `FS-030` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose full upper holds a copy of a lower file |
| When | the file is removed |
| Then | the removal is refused as out of space, and the copy still reads |

## `FS-041` A directory made over a whiteout without room is not made

| Attribute | Value |
| --- | --- |
| relates | `FS-018`, `FS-030` |

| Step | Statement |
| --- | --- |
| Given | an Overlay hiding a removed lower directory, its upper with no room left |
| When | the directory is made again |
| Then | the creation is refused as out of space, and the directory stays hidden |

## `FS-042` A rename through an Overlay that cannot leave its whiteout moves nothing

| Attribute | Value |
| --- | --- |
| relates | `FS-016`, `FS-030` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper has room only to copy a lower file up |
| When | the file is renamed |
| Then | the rename is refused as out of space, and the file stays where it was |

## `FS-043` A directory a lower Layer holds does not move through an Overlay

| Attribute | Value |
| --- | --- |
| relates | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer holds a directory |
| When | the directory is renamed |
| Then | the rename is refused as crossing devices, and nothing moves |

## `FS-044` A rename onto what an Overlay shows is checked against the view

| Attribute | Value |
| --- | --- |
| relates | `FS-043` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer alone shows a directory holding a file |
| When | a file, then an empty directory, is renamed onto that directory |
| Then | each is refused as rename(2) refuses it, and the directory stays as it was |

## `FS-045` A directory moved over a lower one is marked opaque

| Attribute | Value |
| --- | --- |
| refines | `FS-044` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer holds an empty directory |
| When | a directory the upper holds is renamed onto it, and the lower Layer gains a file there |
| Then | the moved directory lists nothing |

## `FS-046` A copy up that fails part way leaves nothing behind

| Attribute | Value |
| --- | --- |
| relates | `FS-009`, `FS-022` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper has less room than a lower file holds |
| When | the lower file is written, copying it up |
| Then | the write is refused as out of space, and the lower file still reads |

## `FS-047` A Bind opens no link

| Attribute | Value |
| --- | --- |
| relates | `FS-007` |

| Step | Statement |
| --- | --- |
| Given | a Bind whose host directory holds a link to a file beside it |
| When | the link is opened |
| Then | the open is refused as a link, and the file is not reached |

## `FS-048` A Bind reports a link a lookup does not follow as a link

| Attribute | Value |
| --- | --- |
| relates | `FS-047` |

| Step | Statement |
| --- | --- |
| Given | a Bind whose host directory holds a link to a file beside it |
| When | the link is looked up, and its directory is listed |
| Then | both report a link |

## `FS-049` An Overlay reports a link its Layer holds as a link

| Attribute | Value |
| --- | --- |
| refines | `FS-048` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer is a Bind holding a link to a file and one to nothing |
| When | each link is looked up through the Overlay, following none |
| Then | each is reported as a link |

## `FS-050` An Overlay removes a link its lower Layer holds as itself

| Attribute | Value |
| --- | --- |
| refines | `FS-010` |
| relates | `FS-049` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Bind Layer holds a link to a directory and one to nothing |
| When | each link is removed as a directory and then as a file |
| Then | the first is refused as no directory, the rest leave the view, and the Layer stays as it was |

## `FS-051` An Overlay refuses a change through a link its lower Layer holds

| Attribute | Value |
| --- | --- |
| refines | `FS-009` |
| relates | `FS-049` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Bind Layer holds a link to a file and a link to nothing |
| When | the guest writes, creates, or renames through one of those links |
| Then | each change is refused as crossing devices, and the target stays |

## `FS-052` An Overlay finds a dangling link where a new entry would go

| Attribute | Value |
| --- | --- |
| relates | `FS-049` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Bind Layer holds a link to nothing |
| When | a file is created there exclusively, or a directory is made there |
| Then | each is refused as already existing |

## `FS-053` A link a Layer holds hides what the Layers below hold there

| Attribute | Value |
| --- | --- |
| refines | `FS-008` |
| relates | `FS-049` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper Bind Layer holds a link to nothing where a lower Layer holds a file |
| When | the path is opened to read through the Overlay |
| Then | the open is refused as a link, and the lower file is not reached |

## `FS-057` A link a Layer holds hides what the Layers below hold beneath it

| Attribute | Value |
| --- | --- |
| refines | `FS-053` |
| relates | `FS-013` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose higher Bind Layer holds `dirlink` as a link to a directory, over a Layer holding `dirlink/x` |
| When | `dirlink` is looked up and opened as a directory through the Overlay |
| Then | it stands as the link, so nothing of the lower `dirlink` shows |

## `FS-058` Removing a file over a lower directory leaves the directory hidden

| Attribute | Value |
| --- | --- |
| refines | `FS-011` |
| relates | `FS-013`, `FS-016` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper holds file `f` over a lower Layer's directory `f` |
| When | `f` is removed through the Overlay |
| Then | `f` is gone from the view, and the lower directory is as it was |

## `FS-059` A directory moves onto one whose upper copy holds only markers

| Attribute | Value |
| --- | --- |
| relates | `FS-016`, `FS-018` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose directory `dir` shows empty, its upper copy holding only the whiteout of a removed lower file |
| When | another directory is renamed onto `dir` through the Overlay |
| Then | the rename succeeds, and `dir` shows only the moved directory's entries |

## `FS-060` A directory made again moves as the upper's own

| Attribute | Value |
| --- | --- |
| refines | `FS-018` |

| Step | Statement |
| --- | --- |
| Given | an Overlay where a lower Layer's directory was removed and made again |
| When | the directory is renamed through the Overlay |
| Then | it moves, and nothing of the lower Layer's directory shows at either path |

## `FS-061` An Overlay refuses a file moved beneath itself as no directory

| Attribute | Value |
| --- | --- |
| relates | `RT-105` |

| Step | Statement |
| --- | --- |
| Given | an Overlay showing the file `dir/file` |
| When | `dir/file` is renamed to `dir/file/x` through the Overlay |
| Then | the rename is refused as no directory |

## `FS-062` A Tmpfs seeded with a list it cannot hold panics

| Attribute | Value |
| --- | --- |
| refines | `FS-001` |

| Step | Statement |
| --- | --- |
| Given | a list naming the root, a file twice, or a file where another path needs a directory |
| When | a Tmpfs is seeded with it |
| Then | the seeding panics |

## `FS-064` A Bind opens a file already there without writing to create it

| Attribute | Value |
| --- | --- |
| relates | `RT-103`, `FS-006` |

| Step | Statement |
| --- | --- |
| Given | a Bind whose host directory holds a file only reading is allowed on |
| When | the file is opened asking to create it but not to write, and so is a missing one |
| Then | the file opens, and the missing one is created |

## `FS-065` A Bind answers a host failure as the error naming it

| Attribute | Value |
| --- | --- |
| relates | `RF-015`, `FS-005` |

| Step | Statement |
| --- | --- |
| Given | a Bind over a host directory |
| When | a file is created under a name longer than the host allows |
| Then | the creation fails as a name too long, not as an unnamed failure |

## `FS-066` A Bind refuses to list a host name that is not UTF-8

| Attribute | Value |
| --- | --- |
| relates | `FS-065`, `FL-018` |

| Step | Statement |
| --- | --- |
| Given | a Bind on a Linux host whose host directory holds a name that is not UTF-8 |
| When | the directory is listed |
| Then | the listing fails as an illegal byte sequence, not as an unnamed failure |

## `FS-067` A Tmpfs tried with a list it cannot hold answers why

| Attribute | Value |
| --- | --- |
| relates | `FS-062` |

| Step | Statement |
| --- | --- |
| Given | a list naming the root, a file twice, or a file where another path needs a directory |
| When | a Tmpfs is tried with it |
| Then | the trial answers the error naming what the list holds wrong |

## `FS-068` A Bind answers a host file too large as too large

| Attribute | Value |
| --- | --- |
| refines | `FS-065` |

| Step | Statement |
| --- | --- |
| Given | a Bind over a host directory |
| When | the host refuses a write because the file would grow too large |
| Then | the write fails as a file too large, not as an unnamed failure |

## `FS-069` A Bind appends at its host file's end in one step

| Attribute | Value |
| --- | --- |
| relates | `FS-036`, `RT-095` |

| Step | Statement |
| --- | --- |
| Given | a Bind file on a unix host, open through two handles |
| When | both handles append at the same time |
| Then | every append keeps its bytes, and none lands over another |

## `FS-070` A Tmpfs gives each directory it makes an identity of its own

| Attribute | Value |
| --- | --- |
| relates | `RT-117` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs holding a directory |
| When | the directory moves, and another is made at its old path |
| Then | the moved directory keeps its identity, and the new one has another |

## `FS-071` A Bind reports the identity its host gives a path

| Attribute | Value |
| --- | --- |
| relates | `RT-117` |

| Step | Statement |
| --- | --- |
| Given | a Bind over a host directory |
| When | a path beneath it is asked for its metadata |
| Then | the metadata carries the device and inode numbers the host reports |

## `FS-072` An Overlay keeps a directory's identity when a write copies it up

| Attribute | Value |
| --- | --- |
| relates | `RT-117` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose directory stands only in a lower Layer |
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

## `FS-078` An Overlay shows a directory over a lower Layer's file on its path

| Attribute | Value |
| --- | --- |
| relates | `FS-057` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper Layer holds `dir/sub`, over a Layer holding a file `dir` |
| When | `dir/sub` is looked up through the Overlay |
| Then | it reads as a directory, as its files open |

## `FS-080` An Overlay rename answers not found for a file its Layer loses

| Attribute | Value |
| --- | --- |
| relates | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay over a Layer whose file is gone after its first lookup |
| When | the file is renamed through the Overlay |
| Then | the rename answers not found |

## `FS-081` A Bind spells a name it matches exactly as given

| Step | Statement |
| --- | --- |
| Given | a Bind over a host directory holding `dir/file` |
| When | `file`, and a name `dir` lacks, are looked up in `dir` |
| Then | each comes back as given |

## `FS-082` A Bind spells a name as a folding host stores it

| Attribute | Value |
| --- | --- |
| refines | `FS-081` |

| Step | Statement |
| --- | --- |
| Given | a Bind over a macOS host directory folding case and Unicode form |
| When | names are looked up in another case and form |
| Then | each name it holds comes back as the host stores it |

## `FS-083` An Overlay spells a name as the Layer showing it stores it

| Attribute | Value |
| --- | --- |
| refines | `FS-081` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer folds case and stores names in lower case |
| When | names it shows, and a new name beside them, are looked up in upper case |
| Then | the names it holds come back in lower case, and the new name as given |

## `FS-084` A Bind spells a name as a folding Linux filesystem lists it

| Attribute | Value |
| --- | --- |
| refines | `FS-081` |
| unverifiable | a Linux filesystem that folds names, such as vfat, needs root to mount |

| Step | Statement |
| --- | --- |
| Given | a Bind over a Linux host directory whose filesystem folds names |
| When | a name is looked up in another case |
| Then | the name comes back as the directory's listing names it |

## `FS-085` A Filesystem without a device of its own takes one no other names

| Attribute | Value |
| --- | --- |
| relates | `FS-070`, `FS-071` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs, and two device numbers taken for Filesystems without one |
| When | their device numbers are compared, with each other and with a host's |
| Then | each differs from the others, and none is one a host reports |

## `FS-086` A directory opened from a Bind keeps its inode from the next one made

| Attribute | Value |
| --- | --- |
| relates | `FS-071` |

| Step | Statement |
| --- | --- |
| Given | a Bind over a Linux host reusing inode numbers, with a directory opened from it |
| When | the directory is removed and another made at its path |
| Then | the new directory takes another inode number |

## `FS-087` A Tmpfs tried with a path through `..` answers why

| Attribute | Value |
| --- | --- |
| refines | `FS-067` |

| Step | Statement |
| --- | --- |
| Given | a list naming a path through `..` |
| When | a Tmpfs is tried with it |
| Then | the trial answers the error naming that path |

## `FS-090` A Bind opens only a file or a directory

| Attribute | Value |
| --- | --- |
| relates | `FS-028` |

| Step | Statement |
| --- | --- |
| Given | a Bind holding a FIFO, and one over a host directory holding a device |
| When | the FIFO and the device are opened |
| Then | each open is refused as permission denied, without waiting |

## `FS-091` A Bind reports a FIFO as of an unknown type

| Attribute | Value |
| --- | --- |
| refines | `FS-090` |

| Step | Statement |
| --- | --- |
| Given | a Bind holding a FIFO |
| When | the FIFO is looked up and listed |
| Then | each answers an unknown type |

## `FS-092` An Overlay neither copies up nor moves a FIFO below it

| Attribute | Value |
| --- | --- |
| refines | `FS-090` |
| relates | `FS-051` |

| Step | Statement |
| --- | --- |
| Given | an Overlay over a lower Bind holding a FIFO |
| When | the FIFO is written or renamed through the Overlay |
| Then | the write is refused as permission denied, the rename as crossing devices |

## `FS-093` A Bind on Windows refuses to append

| Attribute | Value |
| --- | --- |
| refines | `FS-069` |
| relates | `FS-029` |

| Step | Statement |
| --- | --- |
| Given | a Bind file on a Windows host |
| When | it is appended to |
| Then | the append is refused as unsupported, and the file stays as it was |

## `FS-094` A Tmpfs holds each entry under its own name

| Attribute | Value |
| --- | --- |
| refines | `FS-030` |
| relates | `FS-022` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs holding a file in a directory |
| When | the directory is moved into a new directory of its name, many times over |
| Then | each entry still holds only its own name, however deep it lies |

## `FS-095` An Overlay replaces neither a FIFO nor a directory with the other

| Attribute | Value |
| --- | --- |
| refines | `FS-092` |
| relates | `FS-133` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer holds a FIFO and a directory, its upper a directory |
| When | the FIFO is renamed onto the lower directory, and the upper directory onto the FIFO |
| Then | the first is refused as a directory, the second as no directory, and both stay |

## `FS-096` A write of no bytes leaves a Tmpfs file as it was

| Attribute | Value |
| --- | --- |
| relates | `FS-022` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs holding an empty file |
| When | no bytes are written at an offset past its end |
| Then | the write answers zero, and the file stays empty, as Linux pwrite does |

## `FS-099` A Bind opens no link as a directory

| Attribute | Value |
| --- | --- |
| relates | `FS-047` |

| Step | Statement |
| --- | --- |
| Given | a Bind holding a link to a directory |
| When | the link is opened as a directory |
| Then | the open is a link loop, since the runtime follows a link itself |

## `FS-100` An Overlay opens no link as a directory

| Attribute | Value |
| --- | --- |
| relates | `FS-099` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose Bind Layer holds a link to a directory |
| When | the link is opened as a directory, and read |
| Then | the open is a link loop, and the read answers where it points |

## `FS-101` A file opened to read before a copy up reads the copy

| Attribute | Value |
| --- | --- |
| refines | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer's file is open to read |
| When | another open writes the file, copying it up |
| Then | the open file reads what the copy holds |

## `FS-102` A file opened to read before a rename reads the moved copy

| Attribute | Value |
| --- | --- |
| relates | `FS-101` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer's file is open to read |
| When | the file is renamed and the moved copy is written |
| Then | the open file reads what the moved copy holds |

## `FS-103` A file open to read keeps what it held once removed

| Attribute | Value |
| --- | --- |
| relates | `FS-101` |

| Step | Statement |
| --- | --- |
| Given | an Overlay file open to read after a write copied it up |
| When | the file is removed, and a new file at its path is renamed away |
| Then | the open file still reads what it held |

## `FS-105` A directory opened from a Tmpfs follows it when it moves

| Attribute | Value |
| --- | --- |
| relates | `FS-070` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs directory opened as an Inode |
| When | the directory is renamed, and a file is made through the opened one |
| Then | the file stands beneath the directory's new path |

## `FS-106` A Tmpfs directory removed while opened makes nothing more

| Attribute | Value |
| --- | --- |
| relates | `FS-105` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs directory opened as an Inode |
| When | the directory is removed, then made into, listed, and looked up through the opened one |
| Then | the make is not found, the listing is empty, and the lookup finds a directory |

## `FS-107` A Tmpfs directory never moves beneath itself

| Attribute | Value |
| --- | --- |
| relates | `FS-094` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs directory, and a directory beneath it opened as an Inode |
| When | the outer directory is renamed into the opened one |
| Then | the rename answers invalid input, and both stay where they were |

## `FS-108` A directory opened from a Bind follows it when the host moves it

| Attribute | Value |
| --- | --- |
| relates | `FS-105` |

| Step | Statement |
| --- | --- |
| Given | a Bind's host directory opened as an Inode |
| When | the host renames the directory, and a file is made through the opened one |
| Then | the file stands beneath the directory's new host path |

## `FS-109` A rename into a directory of another Filesystem crosses devices

| Attribute | Value |
| --- | --- |
| relates | `FS-133` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs holding a file, and a directory of another Tmpfs |
| When | the file is renamed into that directory |
| Then | the rename answers crossing devices, and the file stays |

## `FS-110` A directory opened from an Overlay follows it when it moves

| Attribute | Value |
| --- | --- |
| relates | `FS-105` |

| Step | Statement |
| --- | --- |
| Given | an Overlay directory, and one beneath it, each opened as an Inode |
| When | the outer directory is renamed through the Overlay, and files are made through both |
| Then | each file stands beneath the outer directory's new path |

## `FS-111` An Overlay directory removed while opened makes nothing more

| Attribute | Value |
| --- | --- |
| relates | `FS-106` |

| Step | Statement |
| --- | --- |
| Given | an Overlay directory opened as an Inode |
| When | the directory is removed and another made at its path, then made into and listed |
| Then | the make is not found, the listing is empty, and the new directory stays empty |

## `FS-112` An Overlay directory keeps its Layer's directory through a change outside the Overlay

| Attribute | Value |
| --- | --- |
| relates | `FS-110` |

| Step | Statement |
| --- | --- |
| Given | an Overlay directory opened as an Inode |
| When | its Layer moves the directory away and makes another in its place, outside the Overlay |
| Then | a read through the opened directory reads the directory it opened |

## `FS-113` A lower Layer's link beneath an upper directory is not merged

| Attribute | Value |
| --- | --- |
| relates | `FS-100` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper Layer holds a directory where a lower Layer holds a link to a directory |
| When | a name the link's target holds is looked up beneath the directory, which is also listed |
| Then | the lookup answers not found, and the listing is empty |

## `FS-114` A Bind reads where a link points

| Attribute | Value |
| --- | --- |
| relates | `FS-047` |

| Step | Statement |
| --- | --- |
| Given | a Bind holding a link and a directory |
| When | each is read as a link |
| Then | the link answers where it points as stored, and the directory is invalid input |

## `FS-115` A Tmpfs keeps a file's times as a tmpfs mounted noatime does

| Attribute | Value |
| --- | --- |
| relates | `FS-001` |

| Step | Statement |
| --- | --- |
| Given | a file made in a Tmpfs |
| When | it is read, and later written |
| Then | its access time stays, and its other two times move to the write |

## `FS-116` A Tmpfs counts a directory's names as a tmpfs does

| Attribute | Value |
| --- | --- |
| relates | `FS-070` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs directory holding a file and two directories |
| When | the directory and the file are read |
| Then | the directory counts four names, and the file one |

## `FS-117` An Overlay reports a merged directory as overlayfs does

| Attribute | Value |
| --- | --- |
| refines | `FS-072` |

| Step | Statement |
| --- | --- |
| Given | a directory two Layers of an Overlay hold, written last in the upper |
| When | the Overlay reads it |
| Then | it counts one name and reports the upper Layer's times |

## `FS-119` A copy up through one Overlay directory is seen through another

| Attribute | Value |
| --- | --- |
| relates | `FS-072` |

| Step | Statement |
| --- | --- |
| Given | a directory only a lower Layer holds, opened twice beneath an Overlay |
| When | a file is written through the first, copying the directory up |
| Then | a read through the second reads the file |

## `FS-120` A write through a moved Overlay directory lands where it went

| Attribute | Value |
| --- | --- |
| refines | `FS-110` |

| Step | Statement |
| --- | --- |
| Given | a directory opened beneath an Overlay, only the upper holding it |
| When | the directory is moved through the Overlay, then written into through the one opened |
| Then | the file lands where the directory moved |

## `FS-122` An Overlay walks a directory one Layer shows without looking it up

| Attribute | Value |
| --- | --- |
| refines | `FS-008` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer alone holds `top/dir/file`, yet answers no lookup of `dir` |
| When | `top/dir/file` is read through the Overlay |
| Then | it reads the file's content |

## `FS-123` An Overlay refuses an upper that is no Tmpfs

| Attribute | Value |
| --- | --- |
| refines | `FS-015` |

| Step | Statement |
| --- | --- |
| Given | a Bind and a Tmpfs |
| When | an Overlay is built with the Bind as its upper over the Tmpfs |
| Then | it is refused as invalid input |

## `FS-124` An Overlay without an upper needs two lower Layers

| Attribute | Value |
| --- | --- |
| refines | `FS-021` |

| Step | Statement |
| --- | --- |
| Given | one Tmpfs |
| When | an Overlay is built with it as its only Layer |
| Then | it is refused as invalid input |

## `FS-125` An Overlay needs a lower Layer

| Attribute | Value |
| --- | --- |
| refines | `FS-008` |

| Step | Statement |
| --- | --- |
| Given | one Tmpfs |
| When | an Overlay is built with it as its upper and no lower Layer |
| Then | it is refused as invalid input |

## `FS-126` An Overlay stacks at most two deep

| Attribute | Value |
| --- | --- |
| refines | `FS-008` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer is an Overlay |
| When | another Overlay is built over it |
| Then | it is refused as invalid input |

## `FS-127` An Overlay takes at most 500 lower Layers

| Attribute | Value |
| --- | --- |
| refines | `FS-008` |

| Step | Statement |
| --- | --- |
| Given | 501 Tmpfs |
| When | an Overlay is built over all of them |
| Then | it is refused as invalid input |

## `FS-128` An Overlay keeps its changes in its Tmpfs's `upper`

| Attribute | Value |
| --- | --- |
| refines | `FS-015` |

| Step | Statement |
| --- | --- |
| Given | an Overlay over a Tmpfs |
| When | a lower file is changed and then removed through the Overlay |
| Then | the Tmpfs holds only `upper` and an empty `work` |

## `FS-129` A copy up keeps the lower file's times

| Attribute | Value |
| --- | --- |
| refines | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Layer holds a file |
| When | the file is opened to write through the Overlay, and nothing is written |
| Then | it reports the lower file's modified time |

## `FS-130` A copy up leaves its directory's times as they were

| Attribute | Value |
| --- | --- |
| refines | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose upper holds a directory a lower Layer fills |
| When | another lower file there is copied up |
| Then | the directory reports the modified time it had |

## `FS-131` An Overlay and a Mount of its upper never wait on each other

| Attribute | Value |
| --- | --- |
| relates | `FS-037` |

| Step | Statement |
| --- | --- |
| Given | an Overlay, and its upper Tmpfs mounted on its own |
| When | a file moves up one directory through the Overlay, and that directory is removed through the other Mount |
| Then | each call returns |

## `FS-132` A removal through an Overlay waits for a change in that directory

| Attribute | Value |
| --- | --- |
| refines | `FS-037` |

| Step | Statement |
| --- | --- |
| Given | an Overlay, and its upper Tmpfs mounted on its own |
| When | an empty directory is removed through the Overlay, and a directory is made in it through the other Mount |
| Then | a directory made stays |

## `FS-133` A Tmpfs replaces only what rename(2) lets a move replace

| Attribute | Value |
| --- | --- |
| relates | `RT-093` |

| Step | Statement |
| --- | --- |
| Given | a Tmpfs holding a file, an empty directory, and a directory holding a file |
| When | the file moves onto each directory, and each directory onto the file and onto the other |
| Then | each is refused as EISDIR, ENOTDIR, or ENOTEMPTY, and nothing moves |

## `FS-134` A directory removed through an Overlay leaves nothing in `work`

| Attribute | Value |
| --- | --- |
| refines | `FS-128` |

| Step | Statement |
| --- | --- |
| Given | an Overlay over a Tmpfs, its lower Layer holding a directory with one file |
| When | the file and then the directory are removed through the Overlay |
| Then | the directory is gone, and the Tmpfs's `work` is empty |
