# Changelog

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.13.1...kobako-sdk-v0.14.0) (2026-08-06)


### Features

* **transport:** let a guest block's exception continue as itself ([c8b1de7](https://github.com/elct9620/kobako/commit/c8b1de7154336fab8fa20c0d7a5b3baaad11d551))
* **transport:** separate a failed exchange from a failed Service ([fb92515](https://github.com/elct9620/kobako/commit/fb92515fc76ffebbb8491b525d53b1725cf97dd0))

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.13.0...kobako-sdk-v0.13.1) (2026-07-30)


### Miscellaneous Chores

* **kobako-sdk:** Synchronize kobako crates versions

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-sdk-v0.12.0...kobako-sdk-v0.13.0) (2026-07-29)


### ⚠ BREAKING CHANGES

* **transport:** let every envelope spell its success arm the same way
* **spec:** separate what a name promises from what the wire promises
* **sdk:** keep the root to what every build of this crate has
* **sdk:** make the wasm engine something a host can actually take out
* **wasmtime:** put every cap in one struct, named the way the field is read
* **runtime:** give the engine contract names an implementer can write
* **transport:** name the core envelope's types after what they carry
* **sdk:** name a yield's arguments the way every other position does
* **sdk:** let the schema spell a Handle and stop there
* **sdk:** bind the object, not the wrapper it needs
* **sdk:** give the bundled schema a module instead of 36 gates
* **sdk:** reach a result's host object without a schema
* **sdk:** let a run verb take whichever payload it is handed
* **sdk:** make the payload codec an optional dependency
* **sdk:** let a yield carry the bytes the block wrote
* **sdk:** hand out Handle ids rather than one schema's spelling
* **sdk:** attribute an invocation without reading its payload
* **wire:** carry a Reply's fault arm on the envelope
* **wire:** route every tier through the one envelope
* **sdk:** let each type say what it is, not where it came from
* **wire:** give the fault concept one name on both sides of the boundary
* **outcome:** raise an unresolved entrypoint as its own error carrying the names it could have been
* **outcome:** keep a Panic's attribution when its diagnostics are unreadable
* **wire:** carry the Outcome over the core envelope
* **wire:** carry the Yield Reply over the core envelope
* **wire:** carry the Run envelope over the core envelope
* **wire:** carry the invocation frames over the core envelope
* **sdk:** make Receiver payload-opaque and add the Value adapter
* **wire:** carry dispatch over the core envelope with an opaque payload

### Features

* **outcome:** keep a Panic's attribution when its diagnostics are unreadable ([8accd40](https://github.com/elct9620/kobako/commit/8accd404c9d9155f19fa0458d7e0650b5d587eb1))
* **outcome:** raise an unresolved entrypoint as its own error carrying the names it could have been ([05b4125](https://github.com/elct9620/kobako/commit/05b41257ca8d4bcb90d6759c6cc7b20582af0661))
* **sdk:** let a host bring its own wasm engine ([70c2475](https://github.com/elct9620/kobako/commit/70c24759ef4cd5785bd1418a6f8d199620f59e2b))
* **sdk:** let a host encode its own `run` payload ([9c459ec](https://github.com/elct9620/kobako/commit/9c459ece49f5c57532c34fd284cfb94ed7261b06))
* **sdk:** let a Receiver be answered outside an invocation ([d59966a](https://github.com/elct9620/kobako/commit/d59966a33370cead6ac4f2b9266454b0fdd47a31))
* **sdk:** let a run verb take whichever payload it is handed ([0050271](https://github.com/elct9620/kobako/commit/0050271bd3c3ce63b7fb1a3e657b2a4c8aaffb6c))
* **sdk:** make Receiver payload-opaque and add the Value adapter ([6eb210e](https://github.com/elct9620/kobako/commit/6eb210ed165f401799aea4faba1f2fb6f79dffa5))
* **sdk:** make the payload codec an optional dependency ([0c7da71](https://github.com/elct9620/kobako/commit/0c7da71bfc0b138a316c9612b23b2944684d652e))
* **sdk:** make the wasm engine something a host can actually take out ([c9f8de6](https://github.com/elct9620/kobako/commit/c9f8de662e6dcd31c8444d9424b1a87ca144f1a6))
* **sdk:** reach a result's host object without a schema ([a0a2c56](https://github.com/elct9620/kobako/commit/a0a2c56ca5efe8daded876cdcfb8cf0ec8dd4e77))
* **wire:** carry a Reply's fault arm on the envelope ([0bce850](https://github.com/elct9620/kobako/commit/0bce850b3416696e92ae6ee12c353c4c21c8e583))
* **wire:** carry dispatch over the core envelope with an opaque payload ([556104b](https://github.com/elct9620/kobako/commit/556104bf86fdf481b5368b70af83eb0add4b2708))
* **wire:** carry the invocation frames over the core envelope ([b6f266a](https://github.com/elct9620/kobako/commit/b6f266a310ff723a1868cff745763d91c0603a5e))
* **wire:** carry the Outcome over the core envelope ([3fa338e](https://github.com/elct9620/kobako/commit/3fa338e4e03b16c4f25f903f1d45f672ab1d015d))
* **wire:** carry the Run envelope over the core envelope ([d7c46ab](https://github.com/elct9620/kobako/commit/d7c46ab64e382f5226c780b39c8bb66ee12213ac))
* **wire:** carry the Yield Reply over the core envelope ([0db7261](https://github.com/elct9620/kobako/commit/0db726189adbc369e32b569cd191861dbba17302))


### Bug Fixes

* **ext:** name the Panic attribution tuple and drop two lint carries ([ab24625](https://github.com/elct9620/kobako/commit/ab24625233c3559c9b3c737182d6614a4e03926b))
* **release:** drop the version from a dev-dependency nothing rewrites ([524e536](https://github.com/elct9620/kobako/commit/524e5365448fedb101571a11de546dddebbfbcb6))


### Documentation

* **spec:** separate what a name promises from what the wire promises ([545bfbd](https://github.com/elct9620/kobako/commit/545bfbd59834bab6d564850d9adafa892cbae005))


### Code Refactoring

* **runtime:** give the engine contract names an implementer can write ([0a41491](https://github.com/elct9620/kobako/commit/0a41491360cf17def8245c0758679e92572b990f))
* **sdk:** attribute an invocation without reading its payload ([70d22d0](https://github.com/elct9620/kobako/commit/70d22d01c79034d3d3011e95af5839d6eea654c8))
* **sdk:** bind the object, not the wrapper it needs ([3baa62c](https://github.com/elct9620/kobako/commit/3baa62cd3f1ae43d429288cfe47f245341fb8eec))
* **sdk:** give the bundled schema a module instead of 36 gates ([b432431](https://github.com/elct9620/kobako/commit/b4324314e8f4343e283d0ce351f7dfaceab6301d))
* **sdk:** hand out Handle ids rather than one schema's spelling ([85b5e9c](https://github.com/elct9620/kobako/commit/85b5e9c3725df4190ebad8ef8b4445ff4f85ab4e))
* **sdk:** keep the root to what every build of this crate has ([7f93096](https://github.com/elct9620/kobako/commit/7f93096d4371e7e7f70f3b31301fad3a3198184a))
* **sdk:** let a yield carry the bytes the block wrote ([d5382d5](https://github.com/elct9620/kobako/commit/d5382d557ee3f840518ef3c9d909b87f3c2acf64))
* **sdk:** let each type say what it is, not where it came from ([d942083](https://github.com/elct9620/kobako/commit/d94208363b1b10f3c34ba22ef96e77fbf33361e5))
* **sdk:** let the schema spell a Handle and stop there ([f48684e](https://github.com/elct9620/kobako/commit/f48684e9e5cd526aeb53335120564ea835042fc1))
* **sdk:** name a yield's arguments the way every other position does ([8eeba6c](https://github.com/elct9620/kobako/commit/8eeba6c781186d7256ee0e282a1fc301cdd4e4f6))
* **transport:** let every envelope spell its success arm the same way ([df595e0](https://github.com/elct9620/kobako/commit/df595e08097af9e3e0a0c5ed52a4a6f854eb21dc))
* **transport:** name the core envelope's types after what they carry ([b44de65](https://github.com/elct9620/kobako/commit/b44de6502953826c567d4ef8a594561479f50c1d))
* **wasmtime:** put every cap in one struct, named the way the field is read ([3f57860](https://github.com/elct9620/kobako/commit/3f578608b044d1ade0ae35231266cf3c9e517c02))
* **wire:** give the fault concept one name on both sides of the boundary ([564798a](https://github.com/elct9620/kobako/commit/564798a9662f547f665530a75d99c73051a79d86))
* **wire:** route every tier through the one envelope ([c5cd33a](https://github.com/elct9620/kobako/commit/c5cd33a5346f49857fa6e1f45c9cf9b9bea0ff77))

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
