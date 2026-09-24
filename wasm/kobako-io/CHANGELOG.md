# Changelog

## [0.18.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.17.0...kobako-io-v0.18.0) (2026-09-24)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.16.0...kobako-io-v0.17.0) (2026-09-20)


### ⚠ BREAKING CHANGES

* **guest:** a pattern's allocator is undefined from the moment the capability installs rather than from its first wrap, so allocating one is refused for the missing allocator throughout an invocation, where before the first statement could still reach an uninitialized carrier.
* **guest:** `Kobako::raise_transport_error`, `raise_service_error` and `reraise` are gone. A flow of its own builds `Kobako::transport_error` or `service_error` and hands the result back as `Err`, which beni raises at the guest call site.

### Features

* **guest:** rebuild the guest crates on beni 0.17 ([b397701](https://github.com/elct9620/kobako/commit/b397701231c7d8718503d24f73b175676d0d1be3))
* **guest:** rebuild the guest crates on beni 0.18 ([ae232eb](https://github.com/elct9620/kobako/commit/ae232ebbf708419ff0672172fbf8627363fb02da))

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.15.0...kobako-io-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.14.0...kobako-io-v0.15.0) (2026-09-13)


### Features

* **mruby:** upgrade beni to 0.14 and locate a parse failure ([586c910](https://github.com/elct9620/kobako/commit/586c910873710169c0dfce1fe7edb21f9e91cdea))

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.13.1...kobako-io-v0.14.0) (2026-08-06)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.13.0...kobako-io-v0.13.1) (2026-07-30)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.12.0...kobako-io-v0.13.0) (2026-07-29)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.11.0...kobako-io-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.10.2...kobako-io-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* release the guest crates at 0.11.0 ([83391c1](https://github.com/elct9620/kobako/commit/83391c15a2bd7b162495e851ad1603a047b0cf0e))

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-io-v0.10.1...kobako-io-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.10.0...kobako-io-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.9.0...kobako-io-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.8.0...kobako-io-v0.9.0) (2026-07-11)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.7.0...kobako-io-v0.8.0) (2026-07-08)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.6.1...kobako-io-v0.7.0) (2026-07-03)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.6.0...kobako-io-v0.6.1) (2026-07-02)


### Bug Fixes

* **guest:** size collection conversions by C array length, not #length ([90ecbd0](https://github.com/elct9620/kobako/commit/90ecbd0cb6a990b8c5a1e5deec3a10df4eaa37df))
* **io:** enforce the fd allowlist at the write syscall ([1b300df](https://github.com/elct9620/kobako/commit/1b300df7bee8f87b701f76b42300163a8899b93e))

## [0.6.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.5.2...kobako-io-v0.6.0) (2026-06-26)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## [0.5.2](https://github.com/elct9620/kobako/compare/kobako-io-v0.5.1...kobako-io-v0.5.2) (2026-06-24)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## [0.5.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.5.0...kobako-io-v0.5.1) (2026-06-14)


### Bug Fixes

* **guest:** adopt beni 0.7.0 protected dispatch (B-51) ([c61655b](https://github.com/elct9620/kobako/commit/c61655bcead336d32a4b6ff7ff1b34c21cdfccd9))

## [0.5.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.4.1...kobako-io-v0.5.0) (2026-06-12)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## [0.4.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.4.0...kobako-io-v0.4.1) (2026-06-11)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## [0.4.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.3.0...kobako-io-v0.4.0) (2026-06-10)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## 0.3.0 (2026-06-08)


### Miscellaneous Chores

* release the guest crates at 0.3.0 ([27a0997](https://github.com/elct9620/kobako/commit/27a099766404cd9c32c54b334dc76d8ec1827675))
