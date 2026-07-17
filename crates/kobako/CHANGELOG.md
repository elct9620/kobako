# Changelog

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.10.0...kobako-sdk-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-sdk:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.9.0...kobako-sdk-v0.10.0) (2026-07-12)


### Features

* **sdk:** add the Extension install mechanism to the Rust host SDK ([4043f76](https://github.com/elct9620/kobako/commit/4043f764b7038619a30c16542ed38d566e4a72a9))

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.8.0...kobako-sdk-v0.9.0) (2026-07-11)


### Features

* **sandbox:** flatten Service registration to path-valued bind ([0876006](https://github.com/elct9620/kobako/commit/0876006455544fd82eb7555ee80c149d98843719))


### Bug Fixes

* **codec:** reject the Fault envelope in Rust host payload positions ([bdf2ed7](https://github.com/elct9620/kobako/commit/bdf2ed78fde2798bdc15f4e969bda228cf482f4b))

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.7.0...kobako-sdk-v0.8.0) (2026-07-08)


### Features

* **crates:** add the kobako host SDK skeleton ([8a99d09](https://github.com/elct9620/kobako/commit/8a99d09ef7068a6738d44f1a735d39516b24156b))
* **crates:** add the parity runner to the kobako SDK ([998f059](https://github.com/elct9620/kobako/commit/998f059abd308ef921c295658aaf8377febb44e2))
* **crates:** grow the SDK capability-Handle table ([f93fe8f](https://github.com/elct9620/kobako/commit/f93fe8f3dce2509cfa527229f8f593f4d816b940))
* **crates:** grow the SDK Member block-yield seam ([4404713](https://github.com/elct9620/kobako/commit/44047130f309a2c935198077fb4f7f86839355e7))
* **crates:** grow the SDK preload and run invocation seams ([d8d5fe2](https://github.com/elct9620/kobako/commit/d8d5fe268a56d45dad4f8b35a25e942a559dcd5f))
* **crates:** honor the respond_to_guest narrowing on the SDK Member seam ([0f5eff1](https://github.com/elct9620/kobako/commit/0f5eff16a1e9f2229ad9d9c9316bf94e92035301))
* **crates:** let a resolved Handle recover its concrete member type ([abd5502](https://github.com/elct9620/kobako/commit/abd55029a44a7631d323c3aec3b625d9692f9c5b))
* **crates:** mark the SDK Error taxonomy non_exhaustive ([001fc69](https://github.com/elct9620/kobako/commit/001fc69e637d2c046f55cb517f9d9cf931793715))


### Bug Fixes

* **crates:** give SetupError a Display so Error::Setup reads cleanly ([bc8b128](https://github.com/elct9620/kobako/commit/bc8b128c9e296319c7fef47441b412d2ce345dff))
