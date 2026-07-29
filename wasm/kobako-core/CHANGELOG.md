# Changelog

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.12.0...kobako-core-v0.13.0) (2026-07-29)


### ⚠ BREAKING CHANGES

* **core:** stop this crate saying "transport" about a different tier
* **transport:** name the core envelope's types after what they carry
* **wire:** carry a Reply's fault arm on the envelope
* **transport:** split the invocation module along the line its doc draws
* **wire:** route every tier through the one envelope
* **sdk:** let each type say what it is, not where it came from
* **guest:** route a dispatch through kobako-core without reading its payload
* **wire:** carry the Outcome over the core envelope
* **wire:** carry the Yield Reply over the core envelope
* **wire:** carry the invocation frames over the core envelope
* **wire:** carry dispatch over the core envelope with an opaque payload

### Features

* **wire:** carry a Reply's fault arm on the envelope ([0bce850](https://github.com/elct9620/kobako/commit/0bce850b3416696e92ae6ee12c353c4c21c8e583))
* **wire:** carry dispatch over the core envelope with an opaque payload ([556104b](https://github.com/elct9620/kobako/commit/556104bf86fdf481b5368b70af83eb0add4b2708))
* **wire:** carry the invocation frames over the core envelope ([b6f266a](https://github.com/elct9620/kobako/commit/b6f266a310ff723a1868cff745763d91c0603a5e))
* **wire:** carry the Outcome over the core envelope ([3fa338e](https://github.com/elct9620/kobako/commit/3fa338e4e03b16c4f25f903f1d45f672ab1d015d))
* **wire:** carry the Yield Reply over the core envelope ([0db7261](https://github.com/elct9620/kobako/commit/0db726189adbc369e32b569cd191861dbba17302))


### Code Refactoring

* **core:** stop this crate saying "transport" about a different tier ([95a5de6](https://github.com/elct9620/kobako/commit/95a5de6bfaf319b43c8d51a52b6b210acd87f1e3))
* **guest:** route a dispatch through kobako-core without reading its payload ([62c5791](https://github.com/elct9620/kobako/commit/62c5791eb88b9021b3d60c8a3dd45d5907011214))
* **sdk:** let each type say what it is, not where it came from ([d942083](https://github.com/elct9620/kobako/commit/d94208363b1b10f3c34ba22ef96e77fbf33361e5))
* **transport:** name the core envelope's types after what they carry ([b44de65](https://github.com/elct9620/kobako/commit/b44de6502953826c567d4ef8a594561479f50c1d))
* **transport:** split the invocation module along the line its doc draws ([4d3a2d3](https://github.com/elct9620/kobako/commit/4d3a2d3060167d71a807e203688186740fd1485b))
* **wire:** route every tier through the one envelope ([c5cd33a](https://github.com/elct9620/kobako/commit/c5cd33a5346f49857fa6e1f45c9cf9b9bea0ff77))

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.11.0...kobako-core-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.10.2...kobako-core-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* release the guest crates at 0.11.0 ([83391c1](https://github.com/elct9620/kobako/commit/83391c15a2bd7b162495e851ad1603a047b0cf0e))

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-core-v0.10.1...kobako-core-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-core-v0.10.0...kobako-core-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.9.0...kobako-core-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.8.0...kobako-core-v0.9.0) (2026-07-11)


### Features

* **sandbox:** flatten Service registration to path-valued bind ([0876006](https://github.com/elct9620/kobako/commit/0876006455544fd82eb7555ee80c149d98843719))

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.7.0...kobako-core-v0.8.0) (2026-07-08)


### Bug Fixes

* **wasm:** guard the frame reader against an over-cap length prefix ([26c6526](https://github.com/elct9620/kobako/commit/26c6526e235d085684c3123b08c5b0319bde1232))

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.6.1...kobako-core-v0.7.0) (2026-07-03)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako crates versions

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-core-v0.6.0...kobako-core-v0.6.1) (2026-07-02)


### Bug Fixes

* **codec:** cap encoder recursion at the nesting depth too ([dfe0ddb](https://github.com/elct9620/kobako/commit/dfe0ddb7e480a491106519ec57f426d9bbce22fa))
* **codec:** reject trailing bytes after a guest envelope value ([36601f1](https://github.com/elct9620/kobako/commit/36601f1eabe97153427d72a07bcd47f20bb07a1f))

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
