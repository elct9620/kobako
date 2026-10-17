# Filesystem

What each Filesystem kobako ships holds, and how long what a guest writes there lasts.

```
guest ─ system interface ─> runtime (descriptors, access, paths) ─> Filesystem
                                                                     these scenarios
```

### Why these scenarios

A Filesystem answers only for files and directories. The runtime in front of it owns descriptors, access, and path resolution. So these scenarios read each kind directly, without a guest. The guest's view through a Grant belongs to the runtime's own scenarios.

Two properties set the kinds apart: where the content lives, and how long it lasts. Each kind is witnessed on both. The Directory kind adds a third property, that nothing outside its directory is reachable.

```
kind       content lives       lasts
Memory     in memory           as long as the Host App's object
Temporary  in memory           one Invocation
Directory  in a host directory as long as the directory
Overlay    in its upper layer  as long as the upper layer
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

## `FS-008` An Overlay reads through to its lower Filesystem

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Filesystem holds a file |
| When | the file is read through the Overlay |
| Then | it reads the lower file's content |

## `FS-009` An Overlay writes into its upper Filesystem

| Attribute | Value |
| --- | --- |
| relates | `FS-008` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Filesystem holds a file |
| When | the file is written through the Overlay |
| Then | the upper Filesystem holds what was written |

## `FS-010` An Overlay leaves its lower Filesystem as it was

| Attribute | Value |
| --- | --- |
| relates | `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Filesystem holds a file |
| When | the file is written and then removed through the Overlay |
| Then | the lower Filesystem still holds the file as it was |

## `FS-011` A file removed through an Overlay is gone from its view

| Attribute | Value |
| --- | --- |
| relates | `FS-010` |

| Step | Statement |
| --- | --- |
| Given | an Overlay whose lower Filesystem holds a file |
| When | the file is removed through the Overlay |
| Then | the Overlay finds no file at that path |

## `FS-012` An Overlay over a Temporary starts every Invocation from its lower Filesystem

| Attribute | Value |
| --- | --- |
| relates | `FS-004`, `FS-009` |

| Step | Statement |
| --- | --- |
| Given | an Overlay over a Temporary, with a lower file overwritten through it |
| When | the next Invocation's Filesystem is taken from it |
| Then | that Filesystem reads the lower file's content |
