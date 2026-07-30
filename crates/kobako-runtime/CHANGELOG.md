# Changelog

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.13.0...kobako-runtime-v0.13.1) (2026-07-30)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.12.0...kobako-runtime-v0.13.0) (2026-07-29)


### ⚠ BREAKING CHANGES

* **runtime:** mark the sets that grow, and say why the closed ones do not
* **spec:** separate what a name promises from what the wire promises
* **runtime:** give the engine contract names an implementer can write
* **sdk:** attribute an invocation without reading its payload
* **wire:** route every tier through the one envelope
* **outcome:** raise an unresolved entrypoint as its own error carrying the names it could have been
* **wire:** carry dispatch over the core envelope with an opaque payload

### Features

* **outcome:** raise an unresolved entrypoint as its own error carrying the names it could have been ([05b4125](https://github.com/elct9620/kobako/commit/05b41257ca8d4bcb90d6759c6cc7b20582af0661))
* **runtime:** mark the sets that grow, and say why the closed ones do not ([1be2449](https://github.com/elct9620/kobako/commit/1be24492961e2c2a3a317f784cc2f20ae584dcbf))
* **wire:** add the core envelope, implemented independently on both peers ([e6c41d1](https://github.com/elct9620/kobako/commit/e6c41d179e5bf619d43d94160dd597c8ae3bc9bf))
* **wire:** carry dispatch over the core envelope with an opaque payload ([556104b](https://github.com/elct9620/kobako/commit/556104bf86fdf481b5368b70af83eb0add4b2708))


### Bug Fixes

* **sig:** declare the dispatch seam the shape the ext actually calls ([f0de465](https://github.com/elct9620/kobako/commit/f0de46500eda366d3edd7869edcc39d843f4b687))


### Documentation

* **spec:** separate what a name promises from what the wire promises ([545bfbd](https://github.com/elct9620/kobako/commit/545bfbd59834bab6d564850d9adafa892cbae005))


### Code Refactoring

* **runtime:** give the engine contract names an implementer can write ([0a41491](https://github.com/elct9620/kobako/commit/0a41491360cf17def8245c0758679e92572b990f))
* **sdk:** attribute an invocation without reading its payload ([70d22d0](https://github.com/elct9620/kobako/commit/70d22d01c79034d3d3011e95af5839d6eea654c8))
* **wire:** route every tier through the one envelope ([c5cd33a](https://github.com/elct9620/kobako/commit/c5cd33a5346f49857fa6e1f45c9cf9b9bea0ff77))

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.11.0...kobako-runtime-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.10.2...kobako-runtime-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.10.1...kobako-runtime-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.10.0...kobako-runtime-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.9.0...kobako-runtime-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.8.0...kobako-runtime-v0.9.0) (2026-07-11)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.7.0...kobako-runtime-v0.8.0) (2026-07-08)


### Bug Fixes

* **crates:** give SetupError a Display so Error::Setup reads cleanly ([bc8b128](https://github.com/elct9620/kobako/commit/bc8b128c9e296319c7fef47441b412d2ce345dff))

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.6.1...kobako-runtime-v0.7.0) (2026-07-03)


### Features

* **crates:** build the requested isolation profile into the WASI context ([63c25d8](https://github.com/elct9620/kobako/commit/63c25d835d4d03010c1658217cee412318e6b5d8))
* **runtime:** runtimes declare their isolation profile ([f89717a](https://github.com/elct9620/kobako/commit/f89717a6bc9809f0de0df78f97da33a49a1474ac))

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.6.0...kobako-runtime-v0.6.1) (2026-07-02)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions
