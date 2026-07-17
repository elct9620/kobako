# kobako-io

Sandbox IO / Kernel capability gem for
[kobako](https://github.com/elct9620/kobako) mruby guests — `$stdout`
/ `$stderr` over wasi-libc.

A `beni::Gem` installing the write-only Ruby IO surface:

- the `::IO` class, the `STDOUT` / `STDERR` constants, and the
  assignable `$stdout` / `$stderr` globals
- the private Kernel output delegators (`print` / `puts` / `printf` /
  `p` / `putc` / `warn`) dispatching through those globals at call
  time

Pure Rust over [beni](https://crates.io/crates/beni) — the whole
Ruby-level surface is defined through the typed wrapper; no mrblib,
no mrbc / RITE pipeline. The gem is kobako-free: output goes straight
to wasi-libc `write(2)`, so any guest shell can compose it.

## Usage

```toml
[dependencies]
kobako-io = "0.10.1" # x-release-please-version
beni = "0.3"
```

```rust
mrb.init_gem::<kobako_io::KobakoIo>()?;
```

In a kobako guest shell the call lives in the `MrbGuest::init_gems`
hook — the in-repo `kobako-wasm` shell composing the bundled
`kobako.wasm` is the worked example.

## License

Apache-2.0
