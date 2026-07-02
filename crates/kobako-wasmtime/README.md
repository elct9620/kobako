# kobako-wasmtime

The [wasmtime](https://wasmtime.dev) implementation of the
[kobako](https://github.com/elct9620/kobako) host runtime contract
([`kobako-runtime`](https://crates.io/crates/kobako-runtime)).

`Driver` implements the contract's `Runtime` trait over wasmtime and
owns every engine-bound mechanic, so frontends see only the neutral
contract surface:

- process-wide Engine and compiled-Module caches with an on-disk AOT
  (`.cwasm`) artifact cache keyed by Guest Binary content
- a pre-linked `InstancePre` per guest path; every invocation runs on
  a fresh instance and discards its Store afterwards
- the epoch-based wall-clock timeout and the per-invocation
  linear-memory cap
- ambient denial: frozen WASI clocks and a constant RNG, so a guest
  observes no real time and no real entropy

The kobako Ruby gem's native ext is the first frontend; a Rust host
SDK consumes the same surface.

## License

Apache-2.0
