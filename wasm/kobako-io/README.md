# kobako-io

Sandbox IO / File / Kernel capability gem for
[kobako](https://github.com/elct9620/kobako) mruby guests — `$stdout`
/ `$stderr` and the host's preopened directories over wasi-libc.

A `beni::Gem` installing the Ruby IO and File surface.

| What it installs | Detail |
|---|---|
| The IO surface | the `::IO` class, the `STDOUT` / `STDERR` constants, and the assignable `$stdout` / `$stderr` globals |
| The File surface | `::File < IO` and `File::Stat` over the directories the host preopens, raising `NotImplementedError` where it preopens none |
| The Dir surface | `::Dir`, listing, making, and removing directories beneath those preopens, unimplemented alike without one |
| The socket surface | `::TCPSocket` over WASI 0.2's `wasi:sockets`, raising `NotImplementedError` where the host gives no network |
| The errors | `IOError`, `EOFError`, `SystemCallError`, and the `Errno` classes a file operation raises |
| The Kernel delegators | `print` / `puts` / `printf` / `p` / `putc` / `warn`, private, dispatching through those globals at call time |

Pure Rust over [beni](https://crates.io/crates/beni) — the whole
Ruby-level surface is defined through the typed wrapper; no mrblib,
no mrbc / RITE pipeline. The gem is kobako-free: it reaches the host
only through WASI, preview1 through wasi-libc and the sockets through WASI
0.2. Any guest shell exporting `cabi_realloc` can compose it.

## Usage

```toml
[dependencies]
kobako-io = "0.20.0" # x-release-please-version
```

```rust
mrb.init_gem::<kobako_io::KobakoIo>()?;
```

In a kobako guest shell the call lives in the `MrbGuest::init_gems`
hook — the in-repo `kobako-wasm` shell composing the bundled
`kobako.wasm` is the worked example.

## License

Licensed under [Apache-2.0](https://github.com/elct9620/kobako/blob/main/LICENSE).
