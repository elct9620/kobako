# Changelog

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
