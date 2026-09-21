# Changelog

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.16.0...kobako-json-v0.17.0) (2026-09-20)


### ⚠ BREAKING CHANGES

* **guest:** a pattern's allocator is undefined from the moment the capability installs rather than from its first wrap, so allocating one is refused for the missing allocator throughout an invocation, where before the first statement could still reach an uninitialized carrier.
* **json:** a JSON failure whose error class the guest replaced reaches the Host App as a guest exception rather than as a trap.
* **json:** JSON.parse refuses a second positional argument. Options passed as a positional Hash, the shape MRI also refuses, must be written as keywords.
* **guest:** `Kobako::raise_transport_error`, `raise_service_error` and `reraise` are gone. A flow of its own builds `Kobako::transport_error` or `service_error` and hands the result back as `Err`, which beni raises at the guest call site.

### Features

* **guest:** rebuild the guest crates on beni 0.17 ([b397701](https://github.com/elct9620/kobako/commit/b397701231c7d8718503d24f73b175676d0d1be3))
* **guest:** rebuild the guest crates on beni 0.18 ([ae232eb](https://github.com/elct9620/kobako/commit/ae232ebbf708419ff0672172fbf8627363fb02da))


### Bug Fixes

* **json:** read a parse's options from the keywords it declares ([6f0c83c](https://github.com/elct9620/kobako/commit/6f0c83cc0b6ae0fe0f327ad0b514d46a6f3432c0))
* **json:** report a replaced error class instead of ending the invocation ([5a75e42](https://github.com/elct9620/kobako/commit/5a75e42ba7990eb31d8487dd3979cca3c4a3d29b))

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.15.0...kobako-json-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.14.0...kobako-json-v0.15.0) (2026-09-13)


### Features

* **mruby:** upgrade beni to 0.14 and locate a parse failure ([586c910](https://github.com/elct9620/kobako/commit/586c910873710169c0dfce1fe7edb21f9e91cdea))

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.13.1...kobako-json-v0.14.0) (2026-08-06)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-json-v0.13.0...kobako-json-v0.13.1) (2026-07-30)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.12.0...kobako-json-v0.13.0) (2026-07-29)


### Bug Fixes

* **guest:** refuse text the capability gems cannot read as text ([539045a](https://github.com/elct9620/kobako/commit/539045a80192cc28c23a4bcbebc311c55eb138fb))

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.11.0...kobako-json-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.10.2...kobako-json-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* release the guest crates at 0.11.0 ([83391c1](https://github.com/elct9620/kobako/commit/83391c15a2bd7b162495e851ad1603a047b0cf0e))

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-json-v0.10.1...kobako-json-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-json-v0.10.0...kobako-json-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.9.0...kobako-json-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.8.0...kobako-json-v0.9.0) (2026-07-11)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.7.0...kobako-json-v0.8.0) (2026-07-08)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.6.1...kobako-json-v0.7.0) (2026-07-03)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-json-v0.6.0...kobako-json-v0.6.1) (2026-07-02)


### Miscellaneous Chores

* **kobako-json:** Synchronize kobako crates versions

## [0.6.0](https://github.com/elct9620/kobako/compare/kobako-json-v0.5.2...kobako-json-v0.6.0) (2026-06-26)


### Features

* **kobako-json:** add the sandbox JSON capability gem ([74e6f79](https://github.com/elct9620/kobako/commit/74e6f79a2e1e68dadf817f8eb402f56a2f5d16ff))
* **kobako-json:** classify generate values by native mruby type ([f8e5de3](https://github.com/elct9620/kobako/commit/f8e5de3a2bdc0234290101734f2cceff88058cd6))


### Bug Fixes

* **kobako-json:** bound untrusted literal in parser error and correct hash comment ([9a88a43](https://github.com/elct9620/kobako/commit/9a88a43c8d91d5e235898e29d50b3ed472aa5388))
