# Changelog

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.11.0...kobako-sdk-v0.12.0) (2026-07-24)


### Features

* **kobako:** declare fillable Service paths in the Rust SDK ([0cdbeb0](https://github.com/elct9620/kobako/commit/0cdbeb00bd58edfbe94b2c7620d9b2e8dd5f0e6a))
* **kobako:** drive concurrent evals through Arc&lt;Sandbox&gt; ([615e806](https://github.com/elct9620/kobako/commit/615e80659fe9104fae837500e1402bd15d48410a))
* **kobako:** give #run a Context override closure via run_with ([b4d9756](https://github.com/elct9620/kobako/commit/b4d975632aa9ed1db1ff5dc8abaebfdfd854cd8e))
* **kobako:** give an Extension backend the fillable third kind ([c95dc1d](https://github.com/elct9620/kobako/commit/c95dc1db201b527074042db0c702d9d2201ae212))
* **kobako:** override declared paths per invocation with eval_with ([840ef3e](https://github.com/elct9620/kobako/commit/840ef3e94739544b31f889f81e806a1a03029b35))
* **kobako:** return an Execution from eval and run ([5caaa51](https://github.com/elct9620/kobako/commit/5caaa513a328a17ad148a49b1c6238ed78add6da))


### Bug Fixes

* **extension:** re-assert dependencies after a failed seal ([72839ba](https://github.com/elct9620/kobako/commit/72839bac6186e81a1d4323bd3f1eb4f10abf3d64))
* **kobako:** make a repeated ctx.bind override last-wins ([0f2c298](https://github.com/elct9620/kobako/commit/0f2c298bc58b8d40284a9ba1d1947b8718ac714c))

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.10.2...kobako-sdk-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* **kobako-sdk:** Synchronize kobako crates versions

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.10.1...kobako-sdk-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-sdk:** Synchronize kobako crates versions

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
