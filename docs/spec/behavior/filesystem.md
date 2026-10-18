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
create   lands in the topmost writable layer
remove   leaves every writable layer; a whiteout hides what a read-only layer shows
nothing writable  every change is refused as read-only

whiteout          .wh.<name>      beside the hidden entry
opaque directory  .wh..wh..opq    inside a directory made again where one was removed
both live in the topmost writable layer, never show, and no name may claim .wh.
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
