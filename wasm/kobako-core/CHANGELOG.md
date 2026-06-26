# Changelog

## [0.6.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.5.2...kobako-core-v0.6.0) (2026-06-26)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako guest crates versions

## [0.5.2](https://github.com/elct9620/kobako/compare/kobako-core-v0.5.1...kobako-core-v0.5.2) (2026-06-24)


### Bug Fixes

* **codec:** cap decoder pre-allocation to the available bytes ([ff4cb37](https://github.com/elct9620/kobako/commit/ff4cb37a7314b95f151c1bf4b7dbb2eff1e775e6))
* **codec:** cap the guest decoder's nesting depth ([71d75ee](https://github.com/elct9620/kobako/commit/71d75eece7cc5648025581cf56657e06c1946352))

## [0.5.1](https://github.com/elct9620/kobako/compare/kobako-core-v0.5.0...kobako-core-v0.5.1) (2026-06-14)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako guest crates versions

## [0.5.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.4.1...kobako-core-v0.5.0) (2026-06-12)


### Features

* **guest:** bake the canonical boot state and instantiate per invocation (B-49) ([ee9ae6e](https://github.com/elct9620/kobako/commit/ee9ae6e09eab30f54dba0eeec00a5a2c80da819f))

## [0.4.1](https://github.com/elct9620/kobako/compare/kobako-core-v0.4.0...kobako-core-v0.4.1) (2026-06-11)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako guest crates versions

## [0.4.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.3.0...kobako-core-v0.4.0) (2026-06-10)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako guest crates versions

## [0.3.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.2.0...kobako-core-v0.3.0) (2026-06-08)


### Miscellaneous Chores

* release the guest crates at 0.3.0 ([27a0997](https://github.com/elct9620/kobako/commit/27a099766404cd9c32c54b334dc76d8ec1827675))

## [0.2.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.1.0...kobako-core-v0.2.0) (2026-06-05)


### Features

* validate the Guest Binary ABI version at Sandbox construction ([63f22de](https://github.com/elct9620/kobako/commit/63f22deb88dc8acfeae56dccdbf31a7b3650da0d))
* **wasm:** turn the Guest ABI into a trait + export_guest! macro ([3532dc2](https://github.com/elct9620/kobako/commit/3532dc20521ca8d9dd55bc39f01ff611d9df0d4b))
