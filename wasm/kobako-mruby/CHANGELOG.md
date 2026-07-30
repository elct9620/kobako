# Changelog

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.13.0...kobako-mruby-v0.13.1) (2026-07-30)


### Performance Improvements

* **mruby:** mix the proxy seam in where the Handle side already does ([2aa1c45](https://github.com/elct9620/kobako/commit/2aa1c45ec48ee8f66612dbbf8fb4ce49357c251e))
* **mruby:** record the boot constant set once, not on every #run ([a7f6bb9](https://github.com/elct9620/kobako/commit/a7f6bb91d20e29aa5cd91c5568274c367a04c8f7))
* **mruby:** resolve a bind path's namespace once per namespace ([55cd1f3](https://github.com/elct9620/kobako/commit/55cd1f30313b70993aae8c973361ef636ce5d454))

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.12.0...kobako-mruby-v0.13.0) (2026-07-29)


### ⚠ BREAKING CHANGES

* **runtime:** mark the sets that grow, and say why the closed ones do not
* **transport:** let every envelope spell its success arm the same way
* **mruby:** name each codec method after the position it serves
* **core:** stop this crate saying "transport" about a different tier
* **transport:** name the core envelope's types after what they carry
* **guest:** make a parked block and its wire bit one statement
* **guest:** let the shell ask for the codec instead of inheriting it
* **wire:** carry a Reply's fault arm on the envelope
* **transport:** split the invocation module along the line its doc draws
* **guest:** give the codec's contact surface with the VM a file
* **wire:** route every tier through the one envelope
* **guest:** let a forwarded refusal say whose it is
* **guest:** name the payload seam for what it is — a codec
* **wire:** keep a Fault to what its author can bound
* **wire:** give the fault concept one name on both sides of the boundary
* **guest:** let the shell name the schema its guest speaks
* **guest:** route a dispatch through kobako-core without reading its payload
* **outcome:** raise an unresolved entrypoint as its own error carrying the names it could have been
* **wire:** carry the Outcome over the core envelope
* **wire:** carry the Yield Reply over the core envelope
* **wire:** carry the Run envelope over the core envelope
* **wire:** carry the invocation frames over the core envelope
* **wire:** carry dispatch over the core envelope with an opaque payload

### Features

* **guest:** let the shell name the schema its guest speaks ([5ad2e0d](https://github.com/elct9620/kobako/commit/5ad2e0db4c645ab56153a81d1623e37e8cd8f5c6))
* **guest:** open the block seam to a capability gem ([c6d6e49](https://github.com/elct9620/kobako/commit/c6d6e49013bd0f850dd4f2c17273103f166294c2))
* **mruby:** let a codec serve the positions it has and refuse the rest ([1d9677d](https://github.com/elct9620/kobako/commit/1d9677dfabfe5a79925b16199abf1f643b718de7))
* **outcome:** raise an unresolved entrypoint as its own error carrying the names it could have been ([05b4125](https://github.com/elct9620/kobako/commit/05b41257ca8d4bcb90d6759c6cc7b20582af0661))
* **runtime:** mark the sets that grow, and say why the closed ones do not ([1be2449](https://github.com/elct9620/kobako/commit/1be24492961e2c2a3a317f784cc2f20ae584dcbf))
* **spec:** anchor what a guest does at a position its codec does not serve ([3a084ea](https://github.com/elct9620/kobako/commit/3a084ea3c60c193038b6dd06d191b14d2ccde4f1))
* **wire:** carry a Reply's fault arm on the envelope ([0bce850](https://github.com/elct9620/kobako/commit/0bce850b3416696e92ae6ee12c353c4c21c8e583))
* **wire:** carry dispatch over the core envelope with an opaque payload ([556104b](https://github.com/elct9620/kobako/commit/556104bf86fdf481b5368b70af83eb0add4b2708))
* **wire:** carry the invocation frames over the core envelope ([b6f266a](https://github.com/elct9620/kobako/commit/b6f266a310ff723a1868cff745763d91c0603a5e))
* **wire:** carry the Outcome over the core envelope ([3fa338e](https://github.com/elct9620/kobako/commit/3fa338e4e03b16c4f25f903f1d45f672ab1d015d))
* **wire:** carry the Run envelope over the core envelope ([d7c46ab](https://github.com/elct9620/kobako/commit/d7c46ab64e382f5226c780b39c8bb66ee12213ac))
* **wire:** carry the Yield Reply over the core envelope ([0db7261](https://github.com/elct9620/kobako/commit/0db726189adbc369e32b569cd191861dbba17302))


### Bug Fixes

* **guest:** make a parked block and its wire bit one statement ([dba5625](https://github.com/elct9620/kobako/commit/dba5625c541c9b54caa89f5937419ec416ec05a8))
* **guest:** read a String's bytes instead of rendering it ([de8c238](https://github.com/elct9620/kobako/commit/de8c238e900830f72d374f83d8c8e5b63cef4384))
* **guest:** refuse text the capability gems cannot read as text ([539045a](https://github.com/elct9620/kobako/commit/539045a80192cc28c23a4bcbebc311c55eb138fb))
* **mruby:** drop the import the outcome tail stopped using ([cca4311](https://github.com/elct9620/kobako/commit/cca4311d04979f1e4309ab1f207f7451a253d8fe))
* **mruby:** stop a missing core class from raising the wrong one quietly ([226a14b](https://github.com/elct9620/kobako/commit/226a14b3184fdd2797a0b3da7946e49a64287196))
* **mruby:** word a carrying failure by the direction it was going ([7d49fd2](https://github.com/elct9620/kobako/commit/7d49fd21672434238119a9ce19bdc1128e89faf5))


### Code Refactoring

* **core:** stop this crate saying "transport" about a different tier ([95a5de6](https://github.com/elct9620/kobako/commit/95a5de6bfaf319b43c8d51a52b6b210acd87f1e3))
* **guest:** give the codec's contact surface with the VM a file ([a8b95d1](https://github.com/elct9620/kobako/commit/a8b95d117385978ec8538e88f68570697e608e9c))
* **guest:** let a forwarded refusal say whose it is ([7d39913](https://github.com/elct9620/kobako/commit/7d399135d05534232fbcec173121c70e09b1e6f3))
* **guest:** let the shell ask for the codec instead of inheriting it ([abc24d3](https://github.com/elct9620/kobako/commit/abc24d3321c614f36f8c01c932169083796a0915))
* **guest:** name the payload seam for what it is — a codec ([b5d90e2](https://github.com/elct9620/kobako/commit/b5d90e26cdea1e0048cb44e48c6543a6bad4a592))
* **guest:** route a dispatch through kobako-core without reading its payload ([62c5791](https://github.com/elct9620/kobako/commit/62c5791eb88b9021b3d60c8a3dd45d5907011214))
* **mruby:** name each codec method after the position it serves ([e2d4b53](https://github.com/elct9620/kobako/commit/e2d4b53dd2d165d4f6312c8e87a5dd936d2f7a2f))
* **transport:** let every envelope spell its success arm the same way ([df595e0](https://github.com/elct9620/kobako/commit/df595e08097af9e3e0a0c5ed52a4a6f854eb21dc))
* **transport:** name the core envelope's types after what they carry ([b44de65](https://github.com/elct9620/kobako/commit/b44de6502953826c567d4ef8a594561479f50c1d))
* **transport:** split the invocation module along the line its doc draws ([4d3a2d3](https://github.com/elct9620/kobako/commit/4d3a2d3060167d71a807e203688186740fd1485b))
* **wire:** give the fault concept one name on both sides of the boundary ([564798a](https://github.com/elct9620/kobako/commit/564798a9662f547f665530a75d99c73051a79d86))
* **wire:** keep a Fault to what its author can bound ([141209d](https://github.com/elct9620/kobako/commit/141209df0f5525853462fddcfaf3584ea530038b))
* **wire:** route every tier through the one envelope ([c5cd33a](https://github.com/elct9620/kobako/commit/c5cd33a5346f49857fa6e1f45c9cf9b9bea0ff77))

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.11.0...kobako-mruby-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-mruby:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.10.2...kobako-mruby-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* release the guest crates at 0.11.0 ([83391c1](https://github.com/elct9620/kobako/commit/83391c15a2bd7b162495e851ad1603a047b0cf0e))

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.10.1...kobako-mruby-v0.10.2) (2026-07-18)


### Bug Fixes

* **guest:** partition dispatch args by Ruby 3 call semantics (B-58) ([59a15a2](https://github.com/elct9620/kobako/commit/59a15a2d524b1ab902bc5e4a2763ffccb485e399))

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
