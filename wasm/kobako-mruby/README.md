# kobako-mruby

Assembled mruby implementation of the kobako Guest ABI — the
interpreter half of [kobako](https://github.com/elct9620/kobako), an
in-process Wasm sandbox for running untrusted mruby scripts from
Ruby.

[kobako-core](https://crates.io/crates/kobako-core) turns the Guest
ABI into a compiler-checked contract; this crate implements that
contract over mruby:

- `MrbGuest` — the harness trait: one required `init_gems` hook
  naming the shell-chosen `beni::Gem` set, plus provided `eval` /
  `run` / `yield_to_block` flows (canonical boot-state acquisition per
  invocation, frame reading, codec conversion, block-yield re-entry)
  and the build-time `bake_boot` hook behind the wizer
  pre-initialization entry
- `KobakoBridge` — the single built-in gem, installed by the provided
  flows themselves: the `Kobako` module, Service / Handle dispatch
  to the host, and the block machinery
- mruby ↔ wire value conversion between `beni` values and the
  [kobako-codec](https://crates.io/crates/kobako-codec) codec

## Usage

A guest shell implements `MrbGuest`, forwards `kobako_core::Guest` to
the inherited flows (the orphan rule keeps that impl in the shell),
and emits the wasm exports:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
kobako-mruby = "0.10.1" # x-release-please-version
kobako-core = "0.10.1" # x-release-please-version
beni = "0.3"
```

```rust
use beni::{Error, Mrb};

struct MyGuest;

impl kobako_mruby::MrbGuest for MyGuest {
    // KobakoBridge is the harness built-in; the hook names only the
    // shell's additional gems — Ok(()) yields a bridge-only guest.
    fn init_gems(_mrb: &Mrb) -> Result<(), Error> {
        Ok(())
    }
}

impl kobako_core::Guest for MyGuest {
    fn eval() {
        <MyGuest as kobako_mruby::MrbGuest>::eval();
    }

    fn run(env: &[u8]) {
        <MyGuest as kobako_mruby::MrbGuest>::run(env);
    }

    fn yield_to_block(req: &[u8]) -> u64 {
        <MyGuest as kobako_mruby::MrbGuest>::yield_to_block(req)
    }
}

kobako_core::export_guest!(MyGuest);

// Build-time pre-initialization entry: ABI v2 hosts expect the
// canonical boot state baked into the artifact, so expose bake_boot
// and run kobako-baker over the linked module.
#[export_name = "wizer.initialize"]
pub extern "C" fn wizer_initialize() {
    <MyGuest as kobako_mruby::MrbGuest>::bake_boot();
}
```

Any provided flow stays overridable by implementing it in the `Guest`
impl instead of forwarding. Capability gems are separate crates wired
through `init_gems` — the in-repo `kobako-wasm` shell composing this
crate with [kobako-io](https://crates.io/crates/kobako-io) into the
bundled `kobako.wasm` is the worked example. Bake the linked module
with [kobako-baker](https://crates.io/crates/kobako-baker) to produce
the shippable artifact.

## Building

Linking a Guest Binary needs the `libmruby.a` archive that `beni-sys`
discovers via `MRUBY_LIB_DIR` + `WASI_SDK_PATH`; the
[beni](https://github.com/elct9620/beni) gem's rake tasks vendor the
toolchain and build the archive. Without a discovered archive the
crate still compiles in placeholder mode (beni's placeholder rule):
mruby-touching operations panic at runtime instead of failing the
build.

## License

Apache-2.0
