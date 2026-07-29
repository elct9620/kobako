# kobako-transport

The fixed tier of the [kobako](https://github.com/elct9620/kobako) wire —
an in-process Wasm sandbox for running untrusted mruby scripts.

kobako is assembled from three parts chosen independently: a host, a
payload codec, and a guest. That works only because two things are the
same in every assembly. This crate is both of them, and nothing else:

- `envelope` — the **core envelope**, the outer frame each message rides
  in: `Call` / `Reply` / `YieldReply` for a dispatch round-trip,
  `Outcome` / `Panic` for how an invocation ended, `Run` / `Preamble` /
  `Snippets` for what an invocation is handed, and the `ErrorRecord` a
  guest failure carries
- `abi` — the values a host and a guest must already agree on to
  exchange a byte: the version, the packed `(ptr, len)` return layout,
  the invocation-channel frame prefix, and the message size cap

The envelope reads a message's routing fields and its ok-versus-fault
tag without decoding a payload byte; everything the resolved method
consumes rides through as an opaque `payload` this layer never reads.
A decoded envelope borrows the buffer it came from, so that payload
reaches its reader as a view rather than a copy.

This crate depends on no other, and every kobako tier depends on it:
`kobako-runtime` and `kobako-wasmtime` on the host, `kobako-core` and
`kobako-mruby` in the guest, and any third-party engine, codec, or
guest that composes against them.

The byte layout is specified in
[`docs/wire/envelope.md`](https://github.com/elct9620/kobako/blob/main/docs/wire/envelope.md);
the golden vectors in this crate are derived from that document.

## Usage

```toml
[dependencies]
kobako-transport = "0.13.0" # x-release-please-version
```

## License

Apache-2.0
