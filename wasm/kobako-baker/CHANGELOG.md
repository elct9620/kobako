# Changelog

## [0.19.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.18.0...kobako-baker-v0.19.0) (2026-10-24)


### Bug Fixes

* **wasmtime:** require wasmtime 49.0.4 for the aarch64 br_table fix ([390e30b](https://github.com/elct9620/kobako/commit/390e30ba51ed4175820f2bb230e3ad5b0ad1e9a5))
* **wasmtime:** take wasmtime 49.0.3 for its sandbox advisories ([7672592](https://github.com/elct9620/kobako/commit/76725923cac50473c7e3a21cc5121e3cbba0cba5))

## [0.18.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.17.0...kobako-baker-v0.18.0) (2026-10-13)


### ⚠ BREAKING CHANGES

* **transport:** the ABI version is 4, so a Guest Binary built for ABI 3 is refused at Sandbox construction. kobako_core::export_guest! emits __kobako_downcall in place of __kobako_eval, __kobako_run, __kobako_yield_to_block and __kobako_take_outcome, and the host import is __kobako_upcall. kobako_core::abi::{take_outcome, write_outcome, write_reply, decode_run} are replaced by serve and answer, and the frames module is no longer public. Reply, YieldReply and Outcome gain an Unsupported arm and take their tags from the shared registry, and kobako_core::DispatchError and kobako::YieldError gain an Unsupported arm. A yield to a guest without block support raises Kobako::UnsupportedError at the Service's yield instead of failing the invocation as a trap.

### Features

* **transport:** carry each operation as a tag on one channel per direction ([c7d574b](https://github.com/elct9620/kobako/commit/c7d574b59a42e58632f2bdc46cc9a8e4c7ac6121)), closes [#73](https://github.com/elct9620/kobako/issues/73)

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.16.0...kobako-baker-v0.17.0) (2026-09-20)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.15.0...kobako-baker-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.14.0...kobako-baker-v0.15.0) (2026-09-13)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.13.1...kobako-baker-v0.14.0) (2026-08-06)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-baker-v0.13.0...kobako-baker-v0.13.1) (2026-07-30)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.12.0...kobako-baker-v0.13.0) (2026-07-29)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.11.0...kobako-baker-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.10.2...kobako-baker-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* release the guest crates at 0.11.0 ([83391c1](https://github.com/elct9620/kobako/commit/83391c15a2bd7b162495e851ad1603a047b0cf0e))

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-baker-v0.10.1...kobako-baker-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-baker-v0.10.0...kobako-baker-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.9.0...kobako-baker-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.8.0...kobako-baker-v0.9.0) (2026-07-11)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.7.0...kobako-baker-v0.8.0) (2026-07-08)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.6.1...kobako-baker-v0.7.0) (2026-07-03)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-baker-v0.6.0...kobako-baker-v0.6.1) (2026-07-02)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako crates versions

## [0.6.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.5.2...kobako-baker-v0.6.0) (2026-06-26)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako guest crates versions

## [0.5.2](https://github.com/elct9620/kobako/compare/kobako-baker-v0.5.1...kobako-baker-v0.5.2) (2026-06-24)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako guest crates versions

## [0.5.1](https://github.com/elct9620/kobako/compare/kobako-baker-v0.5.0...kobako-baker-v0.5.1) (2026-06-14)


### Miscellaneous Chores

* **kobako-baker:** Synchronize kobako guest crates versions

## [0.5.0](https://github.com/elct9620/kobako/compare/kobako-baker-v0.4.1...kobako-baker-v0.5.0) (2026-06-12)


### Features

* **guest:** bake the canonical boot state and instantiate per invocation (B-49) ([ee9ae6e](https://github.com/elct9620/kobako/commit/ee9ae6e09eab30f54dba0eeec00a5a2c80da819f))
