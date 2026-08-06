# Changelog

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.13.1...kobako-wasmtime-v0.14.0) (2026-08-06)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.13.0...kobako-wasmtime-v0.13.1) (2026-07-30)


### Bug Fixes

* **host:** keep nothing in the ABI probe's capture pipes ([a3d6c1c](https://github.com/elct9620/kobako/commit/a3d6c1c8edb9964a7d8ab162b2e379b39dbbddb9))


### Performance Improvements

* **host:** ask an artifact its ABI version once, not once per Sandbox ([693b94f](https://github.com/elct9620/kobako/commit/693b94f878e56815fe3faeae94e5c4d0cd9c8bb6))

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.12.0...kobako-wasmtime-v0.13.0) (2026-07-29)


### ⚠ BREAKING CHANGES

* **wasmtime:** put every cap in one struct, named the way the field is read
* **runtime:** give the engine contract names an implementer can write
* **transport:** name the core envelope's types after what they carry
* **wire:** route every tier through the one envelope
* **wire:** carry dispatch over the core envelope with an opaque payload

### Features

* **wire:** carry dispatch over the core envelope with an opaque payload ([556104b](https://github.com/elct9620/kobako/commit/556104bf86fdf481b5368b70af83eb0add4b2708))


### Code Refactoring

* **runtime:** give the engine contract names an implementer can write ([0a41491](https://github.com/elct9620/kobako/commit/0a41491360cf17def8245c0758679e92572b990f))
* **transport:** name the core envelope's types after what they carry ([b44de65](https://github.com/elct9620/kobako/commit/b44de6502953826c567d4ef8a594561479f50c1d))
* **wasmtime:** put every cap in one struct, named the way the field is read ([3f57860](https://github.com/elct9620/kobako/commit/3f578608b044d1ade0ae35231266cf3c9e517c02))
* **wire:** route every tier through the one envelope ([c5cd33a](https://github.com/elct9620/kobako/commit/c5cd33a5346f49857fa6e1f45c9cf9b9bea0ff77))

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.11.0...kobako-wasmtime-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.10.2...kobako-wasmtime-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.10.1...kobako-wasmtime-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.10.0...kobako-wasmtime-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.9.0...kobako-wasmtime-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.8.0...kobako-wasmtime-v0.9.0) (2026-07-11)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.7.0...kobako-wasmtime-v0.8.0) (2026-07-08)


### Bug Fixes

* **crates:** keep the no-timeout epoch deadline within range ([a3255df](https://github.com/elct9620/kobako/commit/a3255df98a77825bb39b571a60fe6ff83d269d19))

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.6.1...kobako-wasmtime-v0.7.0) (2026-07-03)


### Features

* **crates:** build the requested isolation profile into the WASI context ([63c25d8](https://github.com/elct9620/kobako/commit/63c25d835d4d03010c1658217cee412318e6b5d8))
* **runtime:** runtimes declare their isolation profile ([f89717a](https://github.com/elct9620/kobako/commit/f89717a6bc9809f0de0df78f97da33a49a1474ac))

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.6.0...kobako-wasmtime-v0.6.1) (2026-07-02)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions
