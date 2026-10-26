# Ruby Filesystem

What a Host App reaches when it writes a Filesystem in Ruby and mounts it like the kinds kobako ships.

```
guest File ─> runtime ─> Mount ─> Ruby object: open / stat / children / mkdir / rmdir / unlink / rename
                                     └─ open answers a file: pread / pwrite / append / size / truncate / fsync / close
```

### Why these scenarios

A Filesystem in Ruby is any object answering `open` and `stat`. Its methods take the names of Ruby's own `File`, `Dir`, and `IO`. A Host App can hand a call on to a real `::File`. Every call the guest makes reaches Ruby, and what Ruby answers or raises reaches the guest.

```
a change method missing   -> Errno::EROFS in the script
a file's append missing   -> Errno::ENOTSUP in the script
EOFError raised by pread  -> the end of the file
an Errno the guest names  -> that Errno in the script
another StandardError     -> Errno::EIO in the script, which goes on
anything else raised      -> the run ends, and it raises in the Host App
```

A StandardError belongs to the guest, which may raise it further. Anything else, such as `Interrupt` or a `Timeout`, belongs to the host. The guest cannot rescue it.

## Includes

- `test/e2e/test_ruby_filesystem.rb`
- `test/e2e/test_ruby_filesystem_host_file.rb`
- `test/e2e/test_host_exception.rb`
- `test/e2e/test_ruby_filesystem_errno.rb`
- `test/e2e/test_ruby_filesystem_append.rb`

## `RF-001` A Ruby object answering open and stat mounts as a Filesystem

| Attribute | Value |
| --- | --- |
| relates | `MT-001` |

| Step | Statement |
| --- | --- |
| Given | a Sandbox mounting a Ruby object that answers `open` and `stat` over a file |
| When | the script reads that file |
| Then | it reads the content the object holds |

## `RF-002` A script writes through a Ruby Filesystem

| Attribute | Value |
| --- | --- |
| relates | `RF-001`, `FL-002` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem mounted for reading and writing |
| When | the script writes a file |
| Then | the Ruby object holds what was written |

## `RF-003` A change a Ruby Filesystem does not answer is refused as read-only

| Attribute | Value |
| --- | --- |
| relates | `FL-006` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem with no `unlink`, mounted for reading and writing |
| When | the script deletes a file |
| Then | the delete raises `Errno::EROFS` |

## `RF-004` An Errno a Ruby Filesystem raises reaches the script

| Attribute | Value |
| --- | --- |
| relates | `RF-001` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem whose `open` raises `Errno::EACCES` |
| When | the script opens a file |
| Then | the open raises `Errno::EACCES` |

## `RF-005` Any other StandardError a Ruby Filesystem raises reaches the script as EIO

| Attribute | Value |
| --- | --- |
| relates | `RF-004`, `MT-008` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem whose `open` raises a `RuntimeError` |
| When | the script opens a file |
| Then | the open raises `Errno::EIO` and the script goes on |

## `RF-006` A close a Ruby Filesystem's file refuses fails the script's close

| Attribute | Value |
| --- | --- |
| refines | `FL-010` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file whose Ruby `close` raises `Errno::EACCES` |
| When | the script closes that file |
| Then | the close raises `Errno::EACCES` |

## `RF-007` A file the script holds open outlives the host's garbage collection

| Attribute | Value |
| --- | --- |
| relates | `RF-001` |

| Step | Statement |
| --- | --- |
| Given | a script holding a Ruby Filesystem's file open under a collection at every allocation |
| When | the script reads that file again |
| Then | it reads the file's content |

## `RF-008` A Ruby Filesystem lives as long as the Overlay holding it as a Layer

| Attribute | Value |
| --- | --- |
| relates | `MT-011`, `FS-008` |

| Step | Statement |
| --- | --- |
| Given | an Overlay holding the only reference to a Ruby Filesystem, after a full collection |
| When | the script reads a file through a mount of that Overlay |
| Then | it reads the Ruby Filesystem's content |

## `RF-009` A mounted Ruby Filesystem answers after the host compacts its heap

| Attribute | Value |
| --- | --- |
| relates | `RF-001`, `MT-016` |
| unverifiable | the invocation's own frames pin the object, so no staged compaction moves it |

| Step | Statement |
| --- | --- |
| Given | a Sandbox mounting a Ruby Filesystem that only the mount refers to |
| When | the host compacts its heap between two reads of a file there |
| Then | the second read answers as the first did |

## `RF-010` A Ruby Filesystem's rename is told what may move

| Attribute | Value |
| --- | --- |
| refines | `MT-018` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem answering `rename`, mounted with a block and without one |
| When | the script renames a file beneath each mount |
| Then | `rename` is told `directory: false` beneath the block and `directory: nil` elsewhere |

## `RF-011` A Ruby Filesystem's open is told to follow no link when the guest asks

| Attribute | Value |
| --- | --- |
| relates | `FS-047` |
| unverifiable | the default Guest Binary's File always asks to follow a link |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem mounted for a guest |
| When | the guest opens a path asking to follow no link at its end |
| Then | `open` is handed `File::NOFOLLOW` among its flags |

## `RF-012` A write a Ruby Filesystem overclaims raises EIO

| Attribute | Value |
| --- | --- |
| refines | `RF-005` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem whose file answers more bytes written than it was handed |
| When | the script writes to that file |
| Then | the write raises `Errno::EIO` |

## `RF-013` A real File a Ruby Filesystem opens reads to its end

| Attribute | Value |
| --- | --- |
| refines | `RF-001` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem whose `open` answers a real `::File` |
| When | the script reads the whole file |
| Then | the read answers the file's content |

## `RF-014` What a Ruby Filesystem raises beyond StandardError ends the run

| Attribute | Value |
| --- | --- |
| relates | `RF-005`, `MT-023`, `T-282` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem whose `open` raises `Interrupt` |
| When | the script opens a file and rescues every failure |
| Then | the run raises `Interrupt` in the Host App |

## `RF-015` An Errno the guest names reaches the script as itself

| Attribute | Value |
| --- | --- |
| refines | `RF-004` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem whose `open` raises `Errno::EPERM` |
| When | the script opens a file |
| Then | the open raises `Errno::EPERM`, not `Errno::EACCES` |

## `RF-016` An Errno the guest has no class for reaches the script as EIO

| Attribute | Value |
| --- | --- |
| refines | `RF-004` |
| relates | `RF-005` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem whose `open` raises `Errno::EAGAIN` |
| When | the script opens a file |
| Then | the open raises `Errno::EIO` |

## `RF-017` A Ruby Filesystem's stat names an identity

| Attribute | Value |
| --- | --- |
| refines | `RF-005` |
| relates | `RT-117` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem whose `stat` answers `directory?` and `size` but no `ino` |
| When | the script reads a file |
| Then | the read raises `Errno::EIO` |

## `RF-018` A script appends through a Ruby Filesystem's file

| Attribute | Value |
| --- | --- |
| refines | `RF-002` |
| relates | `RT-095` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem whose file answers `append` with where the file then ends |
| When | the script writes to that file opened to append |
| Then | the bytes land at the file's end through `append` |

## `RF-019` A Ruby Filesystem's file without append refuses an append

| Attribute | Value |
| --- | --- |
| refines | `RF-018` |
| relates | `FS-093` |

| Step | Statement |
| --- | --- |
| Given | a Ruby Filesystem whose file answers `pwrite` and `size` but no `append` |
| When | the script writes to that file opened to append |
| Then | the write raises `Errno::ENOTSUP`, and the file stays as it was |

## `RF-020` A close a Ruby file refuses as unsupported still fails the script's close

| Attribute | Value |
| --- | --- |
| refines | `RF-006` |

| Step | Statement |
| --- | --- |
| Given | a script holding a file whose Ruby `close` raises `Errno::ENOTSUP` |
| When | the script closes that file |
| Then | the close raises `Errno::ENOTSUP`, as a file lacking `close` never does |
