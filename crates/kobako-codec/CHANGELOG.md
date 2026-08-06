# Changelog

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-codec-v0.13.1...kobako-codec-v0.14.0) (2026-08-06)


### Miscellaneous Chores

* **kobako-codec:** Synchronize kobako crates versions

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-codec-v0.13.0...kobako-codec-v0.13.1) (2026-07-30)


### Miscellaneous Chores

* **kobako-codec:** Synchronize kobako crates versions

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-codec-v0.12.0...kobako-codec-v0.13.0) (2026-07-29)


### ⚠ BREAKING CHANGES

* **runtime:** mark the sets that grow, and say why the closed ones do not
* **wire:** carry a Reply's fault arm on the envelope
* **wire:** route every tier through the one envelope
* **wire:** keep a Fault to what its author can bound
* **wire:** give the fault concept one name on both sides of the boundary
* **outcome:** raise an unresolved entrypoint as its own error carrying the names it could have been
* **wire:** carry the Outcome over the core envelope
* **wire:** carry the Yield Reply over the core envelope
* **wire:** carry the Run envelope over the core envelope
* **codec:** make the MessagePack adapter an optional feature
* **wire:** carry dispatch over the core envelope with an opaque payload

### Features

* **codec:** make the MessagePack adapter an optional feature ([effff27](https://github.com/elct9620/kobako/commit/effff278fd220a4d3c8d4e857828d610fe01f48e))
* **outcome:** raise an unresolved entrypoint as its own error carrying the names it could have been ([05b4125](https://github.com/elct9620/kobako/commit/05b41257ca8d4bcb90d6759c6cc7b20582af0661))
* **runtime:** mark the sets that grow, and say why the closed ones do not ([1be2449](https://github.com/elct9620/kobako/commit/1be24492961e2c2a3a317f784cc2f20ae584dcbf))
* **wire:** add the core envelope, implemented independently on both peers ([e6c41d1](https://github.com/elct9620/kobako/commit/e6c41d179e5bf619d43d94160dd597c8ae3bc9bf))
* **wire:** add the MessagePack payload adapter's invocation arguments ([1fb00f3](https://github.com/elct9620/kobako/commit/1fb00f357353b5705018507eaac5231960deaf0a))
* **wire:** carry a Reply's fault arm on the envelope ([0bce850](https://github.com/elct9620/kobako/commit/0bce850b3416696e92ae6ee12c353c4c21c8e583))
* **wire:** carry dispatch over the core envelope with an opaque payload ([556104b](https://github.com/elct9620/kobako/commit/556104bf86fdf481b5368b70af83eb0add4b2708))
* **wire:** carry the Outcome over the core envelope ([3fa338e](https://github.com/elct9620/kobako/commit/3fa338e4e03b16c4f25f903f1d45f672ab1d015d))
* **wire:** carry the Run envelope over the core envelope ([d7c46ab](https://github.com/elct9620/kobako/commit/d7c46ab64e382f5226c780b39c8bb66ee12213ac))
* **wire:** carry the Yield Reply over the core envelope ([0db7261](https://github.com/elct9620/kobako/commit/0db726189adbc369e32b569cd191861dbba17302))


### Code Refactoring

* **wire:** give the fault concept one name on both sides of the boundary ([564798a](https://github.com/elct9620/kobako/commit/564798a9662f547f665530a75d99c73051a79d86))
* **wire:** keep a Fault to what its author can bound ([141209d](https://github.com/elct9620/kobako/commit/141209df0f5525853462fddcfaf3584ea530038b))
* **wire:** route every tier through the one envelope ([c5cd33a](https://github.com/elct9620/kobako/commit/c5cd33a5346f49857fa6e1f45c9cf9b9bea0ff77))

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-codec-v0.11.0...kobako-codec-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-codec:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-codec-v0.10.2...kobako-codec-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* **kobako-codec:** Synchronize kobako crates versions

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-codec-v0.10.1...kobako-codec-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-codec:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-codec-v0.10.0...kobako-codec-v0.10.1) (2026-07-17)


### Bug Fixes

* **codec:** tighten map-decode pre-allocation to the true pair bound ([7094f91](https://github.com/elct9620/kobako/commit/7094f91898dae4c91c4e6863b09d25a6f34d096e))

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-codec-v0.9.0...kobako-codec-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-codec:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-codec-v0.8.0...kobako-codec-v0.9.0) (2026-07-11)


### Bug Fixes

* **codec:** reject ext 0x02 anywhere in the Panic frame, not only details ([062e29d](https://github.com/elct9620/kobako/commit/062e29d6ee15264e1bd942502b751cfe7610acad))
* **codec:** reject the Fault envelope in Rust host payload positions ([bdf2ed7](https://github.com/elct9620/kobako/commit/bdf2ed78fde2798bdc15f4e969bda228cf482f4b))
* **codec:** reject the reserved Handle id 0 on the Rust wire tier ([5f7e482](https://github.com/elct9620/kobako/commit/5f7e4821680e553da355d5257b0619e4a1cdce72))

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-codec-v0.7.0...kobako-codec-v0.8.0) (2026-07-08)


### Features

* **codec:** add the Run invocation envelope to the wire tier ([dbdd760](https://github.com/elct9620/kobako/commit/dbdd760ddc258681669f7f620f63b75f36322687))


### Bug Fixes

* **crates:** reject trailing bytes on Request and Run decode ([8e4929b](https://github.com/elct9620/kobako/commit/8e4929b64f3ea690f38211888070c2511da84754))

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-codec-v0.6.1...kobako-codec-v0.7.0) (2026-07-03)


### Miscellaneous Chores

* **kobako-codec:** Synchronize kobako crates versions
