# Changelog

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.10.0...kobako-mruby-v0.10.1) (2026-07-17)


### Bug Fixes

* **guest:** reject non-representable dispatch args instead of to_s (E-55) ([6b9ab56](https://github.com/elct9620/kobako/commit/6b9ab562753407cf6168a5baad34140afd75a86a))

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.9.0...kobako-mruby-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-mruby:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.8.0...kobako-mruby-v0.9.0) (2026-07-11)


### Features

* **sandbox:** flatten Service registration to path-valued bind ([0876006](https://github.com/elct9620/kobako/commit/0876006455544fd82eb7555ee80c149d98843719))

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.7.0...kobako-mruby-v0.8.0) (2026-07-08)


### Miscellaneous Chores

* **kobako-mruby:** Synchronize kobako crates versions

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-rs-v0.6.1...kobako-rs-v0.7.0) (2026-07-03)


### Miscellaneous Chores

* **kobako-rs:** Synchronize kobako crates versions

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-rs-v0.6.0...kobako-rs-v0.6.1) (2026-07-02)


### Bug Fixes

* **codec:** reject trailing bytes after a guest envelope value ([36601f1](https://github.com/elct9620/kobako/commit/36601f1eabe97153427d72a07bcd47f20bb07a1f))
* **guest:** size collection conversions by C array length, not #length ([90ecbd0](https://github.com/elct9620/kobako/commit/90ecbd0cb6a990b8c5a1e5deec3a10df4eaa37df))

## [0.6.0](https://github.com/elct9620/kobako/compare/kobako-rs-v0.5.2...kobako-rs-v0.6.0) (2026-06-26)


### Miscellaneous Chores

* **kobako-rs:** Synchronize kobako guest crates versions

## [0.5.2](https://github.com/elct9620/kobako/compare/kobako-rs-v0.5.1...kobako-rs-v0.5.2) (2026-06-24)


### Bug Fixes

* **codec:** cap the guest decoder's nesting depth ([71d75ee](https://github.com/elct9620/kobako/commit/71d75eece7cc5648025581cf56657e06c1946352))
* **codec:** encode guest Handle args/kwargs as ext 0x01 ([bd58538](https://github.com/elct9620/kobako/commit/bd58538f4dbbf91a0927d15fd37f47abc761f8a6))
* **codec:** refuse out-of-range inbound integers instead of saturating ([f9e9184](https://github.com/elct9620/kobako/commit/f9e91845e0f28fecbb0867d8b70c871cd1feafea))
* **dispatch:** keep short method names intact across kwarg unpacking ([c6e4a6f](https://github.com/elct9620/kobako/commit/c6e4a6f268970c0c2d2851d3a23e3bec153dc56d))

## [0.5.1](https://github.com/elct9620/kobako/compare/kobako-rs-v0.5.0...kobako-rs-v0.5.1) (2026-06-14)


### Bug Fixes

* **guest:** adopt beni 0.7.0 protected dispatch (B-51) ([c61655b](https://github.com/elct9620/kobako/commit/c61655bcead336d32a4b6ff7ff1b34c21cdfccd9))

## [0.5.0](https://github.com/elct9620/kobako/compare/kobako-rs-v0.4.1...kobako-rs-v0.5.0) (2026-06-12)


### Features

* **guest:** bake the canonical boot state and instantiate per invocation (B-49) ([ee9ae6e](https://github.com/elct9620/kobako/commit/ee9ae6e09eab30f54dba0eeec00a5a2c80da819f))

## [0.4.1](https://github.com/elct9620/kobako/compare/kobako-rs-v0.4.0...kobako-rs-v0.4.1) (2026-06-11)


### Bug Fixes

* **wasm:** mirror the reflection rejection in the guest proxy ([f6ead3b](https://github.com/elct9620/kobako/commit/f6ead3b91f1ac92c3c075397d177edb4b82cd15d))

## [0.4.0](https://github.com/elct9620/kobako/compare/kobako-rs-v0.3.0...kobako-rs-v0.4.0) (2026-06-10)


### Miscellaneous Chores

* **kobako-rs:** Synchronize kobako guest crates versions

## 0.3.0 (2026-06-08)


### Miscellaneous Chores

* release the guest crates at 0.3.0 ([27a0997](https://github.com/elct9620/kobako/commit/27a099766404cd9c32c54b334dc76d8ec1827675))
