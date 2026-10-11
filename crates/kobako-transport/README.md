# kobako-transport

The fixed tier of the [kobako](https://github.com/elct9620/kobako) wire —
an in-process Wasm sandbox for running untrusted mruby scripts.

kobako is assembled from three parts chosen independently: a host, a
payload codec, and a guest. That works only because two things are the
same in every assembly. This crate is both of them, and nothing else.

| Module | What it fixes |
|---|---|
| `envelope` | the **core envelope**, the outer frame each message rides in |
| `abi` | the values both sides agree on before exchanging a byte |

The envelope covers the downcall's and the upcall's requests under one
tag registry. It also covers a dispatch round-trip, how an invocation
ended, and what an invocation is handed, and re-exports every message
type. The ABI values are the version, the packed `(ptr, len)` return
layout, the stdin frame length prefix, and the message size cap.

## Opaque Payload

The envelope reads a message's routing fields and its answer's tag
without decoding a payload byte. Everything the resolved method consumes
rides through opaque. A decoded envelope borrows the buffer it came from,
so that payload reaches its reader as a view rather than a copy.

```text
Call { method, payload }
                  └── opaque here; read by whoever owns the schema
```

## Dependency Position

This crate depends on no other, and every kobako tier depends on it —
including any third-party engine, codec, or guest composing against them.

```text
host   kobako-runtime · kobako-wasmtime ──┐
guest  kobako-core · kobako-mruby ────────┴──> kobako-transport
```

The golden vectors in this crate are derived from the kobako repository's
byte-layout document, not from this code.

## Usage

Depend on it directly only to build a tier of your own; a guest shell
reaches the same types through `kobako-core`.

```toml
[dependencies]
kobako-transport = "0.17.0" # x-release-please-version
```

## License

Licensed under [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0).
