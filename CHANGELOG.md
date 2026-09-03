# Changelog

## [0.24.0](https://github.com/elct9620/kobako/compare/v0.23.0...v0.24.0) (2026-09-03)


### Features

* **spec:** declare how a dispatch binds its arguments and reports what will not ([d5af2d8](https://github.com/elct9620/kobako/commit/d5af2d8c863f91c7916d9c5ed4ee8ac415c849b1))
* **spec:** declare the boundary's value fidelity and its outbound refusals ([505845c](https://github.com/elct9620/kobako/commit/505845cb02e6d5dd94886cc2256708a9058b4c7f))
* **spec:** declare the options a runtime will not build with ([1bbd018](https://github.com/elct9620/kobako/commit/1bbd018c05099b0501334292dd03e17ffb040c46))
* **spec:** declare the three ways a yield ends badly ([c1b9024](https://github.com/elct9620/kobako/commit/c1b9024416d5c3a2f52c6137621a2656cf95fd1d))
* **spec:** declare what a Pool refuses and what it raises ([b5cec2d](https://github.com/elct9620/kobako/commit/b5cec2d2d1083d49cdfd8ab5dc46ef164ea30aa7))
* **spec:** declare what a preload and an entrypoint refuse ([299c177](https://github.com/elct9620/kobako/commit/299c1771cfaef57023b277bfd01a347f23908354))
* **spec:** declare what a run refuses before the guest is reached ([073f801](https://github.com/elct9620/kobako/commit/073f801f1b37a82d4b6bfd9f574f2c2cce49889c))
* **spec:** declare what an invocation settles into and who it is attributed to ([02a5b38](https://github.com/elct9620/kobako/commit/02a5b38eb256027db53aaa0c8dc5a97d4e2ae927))
* **spec:** declare what bind refuses ([1b5dce3](https://github.com/elct9620/kobako/commit/1b5dce30eb418a1a2deb6e690d43ba3a4d04a675))
* **spec:** declare what install refuses and when the check fires ([1032921](https://github.com/elct9620/kobako/commit/1032921f7e5d1e1c53fbf2d1c6ec46240d1ff768))
* **spec:** declare what the payload wire carries and what it will not ([341fd40](https://github.com/elct9620/kobako/commit/341fd40ce60373858002205ad5a77618e6cbb520))
* **spec:** make Markdown the specification's source ([8d4e56b](https://github.com/elct9620/kobako/commit/8d4e56b18ae8118a2251f60fdbd8b917e5f60ebe))
* **spec:** register the calls each frontend keeps ([dc2285e](https://github.com/elct9620/kobako/commit/dc2285edd275185be35700cd3aec67abd253b8c7))


### Bug Fixes

* **release:** let a gem-only release create its tag ([d65c451](https://github.com/elct9620/kobako/commit/d65c4513d30aacb11d0c008d28791355d8da13b7))
* **spec:** keep the two record-free arms cited on the anchor track ([9eec1b6](https://github.com/elct9620/kobako/commit/9eec1b6696c8ff0d5e1d63bb29c9637dd52bdb52))

## [0.23.0](https://github.com/elct9620/kobako/compare/v0.22.0...v0.23.0) (2026-08-28)


### Features

* **spec:** declare the boot state two invocations share, and what a callback raise costs ([d94fffc](https://github.com/elct9620/kobako/commit/d94fffc59a3142cb28e0025a0dc8c792f7282074))
* **spec:** declare the dispatch boundary one observation at a time ([f11b280](https://github.com/elct9620/kobako/commit/f11b280ef503c20d5b75d7b1c4187d261f60ef27))
* **spec:** declare the guest's pattern surface one observation at a time ([a477818](https://github.com/elct9620/kobako/commit/a477818f8fa505fdcd8ea1fd1051a74c0ba08155))
* **spec:** declare the Pool's behavior where its tests can claim it ([5ec5e4b](https://github.com/elct9620/kobako/commit/5ec5e4b41f44db05520a251ff6979e71efd27866))
* **spec:** declare the two descriptors a guest may write to, and how ([c15d09a](https://github.com/elct9620/kobako/commit/c15d09ab563947ab02bbe31129e88686fcff5c24))
* **spec:** declare the value shapes and the generated concurrency programs ([a06ae5b](https://github.com/elct9620/kobako/commit/a06ae5b24b4615c22c2abaae08c7029218da08bd))
* **spec:** declare what a Sandbox is built with and what each run leaves behind ([8cc6ac8](https://github.com/elct9620/kobako/commit/8cc6ac81713da7fc6ba6c89bc24da75406579aec))
* **spec:** declare what installing an Extension composes and resolves ([3cce5a6](https://github.com/elct9620/kobako/commit/3cce5a6551ae8710df71d7520a689c13630ee111))
* **spec:** declare what the guest's JSON surface reads, writes and refuses ([7e9e3b3](https://github.com/elct9620/kobako/commit/7e9e3b3897ae5f5dc7d6caadeb650302841aa0a6))
* **spec:** declare what the host checks before a guest runs ([28d5d4b](https://github.com/elct9620/kobako/commit/28d5d4bac0efba2aced1b4449926bd868787bd75))
* **spec:** declare where a host object becomes a name the guest can reach ([5d88647](https://github.com/elct9620/kobako/commit/5d88647aa922e3a7bb6915864aec700423060358))


### Bug Fixes

* **gate:** keep the rendered specifications out of the anchor corpus ([1a24c7f](https://github.com/elct9620/kobako/commit/1a24c7ff1367228998b2f21beca885ab7096a893))
* **gate:** stop reading a sumi claim as an anchor citation ([b3c74d6](https://github.com/elct9620/kobako/commit/b3c74d6a296dfb7fa5e157ca9ba4678097bfab43))
* **spec:** let the dispatch feature reach the test that witnesses it ([430394f](https://github.com/elct9620/kobako/commit/430394fa906bf79b8d86d28de7b03e32748516a1))
* **transport:** keep a bound Class's class-level surface off the guest boundary ([dc00355](https://github.com/elct9620/kobako/commit/dc0035524f2a7b3eba3ec378f8f065c19a17660d))

## [0.22.0](https://github.com/elct9620/kobako/compare/v0.21.1...v0.22.0) (2026-08-06)


### Features

* **spec:** give the ubiquitous language a place N-6 can be checked from ([ab44dba](https://github.com/elct9620/kobako/commit/ab44dba0b08e558a5b96b95b3c4d1c70bf6bc7ad))
* **transport:** give a failed Service call a class that says why ([b39f176](https://github.com/elct9620/kobako/commit/b39f1761e9922b7ac761571393f9a6f4478ae522))
* **transport:** let a guest block's exception continue as itself ([c8b1de7](https://github.com/elct9620/kobako/commit/c8b1de7154336fab8fa20c0d7a5b3baaad11d551))
* **transport:** separate a failed exchange from a failed Service ([fb92515](https://github.com/elct9620/kobako/commit/fb92515fc76ffebbb8491b525d53b1725cf97dd0))


### Bug Fixes

* **codec:** keep a value the packer cannot walk inside the codec taxonomy ([ac99d58](https://github.com/elct9620/kobako/commit/ac99d58f0c9a274b230d645d7c37aba88516eab2))
* **regexp:** stop raising where MRI answers no match ([d229974](https://github.com/elct9620/kobako/commit/d2299740a2a0206c88e7c397cdb72dc730c4c1cf))
* **transport:** answer a held block failure only to the block that raised ([24dfd6e](https://github.com/elct9620/kobako/commit/24dfd6ec999e0ba1587b9c69da6d2b29563fc875))
* **transport:** report a yield the host cannot write as the Service's ([19459f8](https://github.com/elct9620/kobako/commit/19459f87555190043127290441bf6b95bcad93d6))
* **transport:** report an answer the host cannot write as the Service's ([bff4cdc](https://github.com/elct9620/kobako/commit/bff4cdccc076859d7da43f5dedb9a283ffed94ee))
* **transport:** spend a block's failure when its Service yields again ([3042a72](https://github.com/elct9620/kobako/commit/3042a72b9e3bb0d6a1d5cb5734d7eaee6a669e85))
* **wire:** let a Fault reader survive what it predates ([eeabfb7](https://github.com/elct9620/kobako/commit/eeabfb7aa4de3d08852ca1b41517b924d72a7312))

## [0.21.1](https://github.com/elct9620/kobako/compare/v0.21.0...v0.21.1) (2026-07-30)


### Bug Fixes

* **bench:** let a re-run stamp its own capture, not the round before it ([3a9793e](https://github.com/elct9620/kobako/commit/3a9793e16c21162ae89c61c4e709cd602fc7ab9f))
* **bench:** move the anchor onto the round the envelope's new tier produced ([ddcfcc1](https://github.com/elct9620/kobako/commit/ddcfcc162b786868807afdf80a284db5b40dc3cd))


### Performance Improvements

* **host:** ask an artifact its ABI version once, not once per Sandbox ([693b94f](https://github.com/elct9620/kobako/commit/693b94f878e56815fe3faeae94e5c4d0cd9c8bb6))
* **mruby:** record the boot constant set once, not on every #run ([a7f6bb9](https://github.com/elct9620/kobako/commit/a7f6bb91d20e29aa5cd91c5568274c367a04c8f7))
* **mruby:** resolve a bind path's namespace once per namespace ([55cd1f3](https://github.com/elct9620/kobako/commit/55cd1f30313b70993aae8c973361ef636ce5d454))

## [0.21.0](https://github.com/elct9620/kobako/compare/v0.20.0...v0.21.0) (2026-07-29)


### ⚠ BREAKING CHANGES

* **spec:** speak one word for the arm every envelope answers success on
* **transport:** let every envelope spell its success arm the same way
* **spec:** separate what a name promises from what the wire promises
* **sdk:** keep the root to what every build of this crate has
* **sdk:** make the wasm engine something a host can actually take out
* **mruby:** name each codec method after the position it serves
* **wasmtime:** put every cap in one struct, named the way the field is read
* **runtime:** give the engine contract names an implementer can write
* **transport:** name the core envelope's types after what they carry
* **guest:** make a parked block and its wire bit one statement
* **sdk:** reach a result's host object without a schema
* **guest:** let the shell ask for the codec instead of inheriting it
* **customization:** the harness carries no codec until a shell asks
* **sdk:** make the payload codec an optional dependency
* **wire:** carry a Reply's fault arm on the envelope
* **wire:** move the Reply's fault arm onto the envelope
* **transport:** split the invocation module along the line its doc draws
* **wire:** route every tier through the one envelope
* **gem:** follow Ruby's own convention for what an error is called
* **guest:** name the payload seam for what it is — a codec
* **wire:** keep a Fault to what its author can bound
* **wire:** give the fault concept one name on both sides of the boundary
* **guest:** let the shell name the schema its guest speaks
* **guest:** route a dispatch through kobako-core without reading its payload
* **outcome:** raise an unresolved entrypoint as its own error carrying the names it could have been
* **outcome:** keep a Panic's attribution when its diagnostics are unreadable
* **wire:** carry the Outcome over the core envelope
* **wire:** carry the Yield Reply over the core envelope
* **wire:** carry the Run envelope over the core envelope
* **wire:** carry the invocation frames over the core envelope
* **codec:** make the MessagePack adapter an optional feature
* **wire:** carry dispatch over the core envelope with an opaque payload

### Features

* **bench:** measure the host's per-invocation cost against a null guest ([c60abdf](https://github.com/elct9620/kobako/commit/c60abdfc98a38b03a4a598913df9a1120ed6302b))
* **bench:** move the two host-side benchmarks into the gated set ([31a6f7e](https://github.com/elct9620/kobako/commit/31a6f7ebb40c038b39c38ee2441216a412be48d0))
* **codec:** make the MessagePack adapter an optional feature ([effff27](https://github.com/elct9620/kobako/commit/effff278fd220a4d3c8d4e857828d610fe01f48e))
* **guest:** let the shell name the schema its guest speaks ([5ad2e0d](https://github.com/elct9620/kobako/commit/5ad2e0db4c645ab56153a81d1623e37e8cd8f5c6))
* **guest:** open the block seam to a capability gem ([c6d6e49](https://github.com/elct9620/kobako/commit/c6d6e49013bd0f850dd4f2c17273103f166294c2))
* **outcome:** keep a Panic's attribution when its diagnostics are unreadable ([8accd40](https://github.com/elct9620/kobako/commit/8accd404c9d9155f19fa0458d7e0650b5d587eb1))
* **outcome:** raise an unresolved entrypoint as its own error carrying the names it could have been ([05b4125](https://github.com/elct9620/kobako/commit/05b41257ca8d4bcb90d6759c6cc7b20582af0661))
* **sdk:** let a host bring its own wasm engine ([70c2475](https://github.com/elct9620/kobako/commit/70c24759ef4cd5785bd1418a6f8d199620f59e2b))
* **sdk:** make the payload codec an optional dependency ([0c7da71](https://github.com/elct9620/kobako/commit/0c7da71bfc0b138a316c9612b23b2944684d652e))
* **sdk:** make the wasm engine something a host can actually take out ([c9f8de6](https://github.com/elct9620/kobako/commit/c9f8de662e6dcd31c8444d9424b1a87ca144f1a6))
* **sdk:** reach a result's host object without a schema ([a0a2c56](https://github.com/elct9620/kobako/commit/a0a2c56ca5efe8daded876cdcfb8cf0ec8dd4e77))
* **spec:** anchor what a guest does at a position its codec does not serve ([3a084ea](https://github.com/elct9620/kobako/commit/3a084ea3c60c193038b6dd06d191b14d2ccde4f1))
* **tasks:** gate signatures against declarations the code dropped ([8b1b834](https://github.com/elct9620/kobako/commit/8b1b834664a51ed6330ba77e9cc3fb7cc4490faf))
* **tasks:** gate the benchmark probes on still running ([31c3455](https://github.com/elct9620/kobako/commit/31c345534c36f49670075fbfdecff645314d6d24))
* **wire:** carry a Reply's fault arm on the envelope ([0bce850](https://github.com/elct9620/kobako/commit/0bce850b3416696e92ae6ee12c353c4c21c8e583))
* **wire:** carry dispatch over the core envelope with an opaque payload ([556104b](https://github.com/elct9620/kobako/commit/556104bf86fdf481b5368b70af83eb0add4b2708))
* **wire:** carry the invocation frames over the core envelope ([b6f266a](https://github.com/elct9620/kobako/commit/b6f266a310ff723a1868cff745763d91c0603a5e))
* **wire:** carry the Outcome over the core envelope ([3fa338e](https://github.com/elct9620/kobako/commit/3fa338e4e03b16c4f25f903f1d45f672ab1d015d))
* **wire:** carry the Run envelope over the core envelope ([d7c46ab](https://github.com/elct9620/kobako/commit/d7c46ab64e382f5226c780b39c8bb66ee12213ac))
* **wire:** carry the Yield Reply over the core envelope ([0db7261](https://github.com/elct9620/kobako/commit/0db726189adbc369e32b569cd191861dbba17302))


### Bug Fixes

* **bench:** bound and surface the band the archive widens ([cc2ddd2](https://github.com/elct9620/kobako/commit/cc2ddd213d8f9382c9b7ff20581abd2b171fe486))
* **bench:** let probe discovery reach any depth ([2a164ca](https://github.com/elct9620/kobako/commit/2a164cab782a51e9d15b522f8a2c340b5f234b9a))
* **bench:** name only the rows the archive actually sets the bar for ([68e167f](https://github.com/elct9620/kobako/commit/68e167f56c801ad4b9caf7f290824d28f216aec9))
* **bench:** sample the cold-start row's guest budget instead of observing it once ([95bdadd](https://github.com/elct9620/kobako/commit/95bdadde1f81fb169293c89cbccd460fb0c6e502))
* **bench:** widen the gate's noise band by what the archive shows ([183407c](https://github.com/elct9620/kobako/commit/183407cfc3854c1179f7c5b7da338d2295526bfc))
* **build:** rebuild the guest when the fixed tier changes ([037cc2e](https://github.com/elct9620/kobako/commit/037cc2e9451fbf34bd2138abfbc8b9b59fd613cc))
* **ext:** name the Panic attribution tuple and drop two lint carries ([ab24625](https://github.com/elct9620/kobako/commit/ab24625233c3559c9b3c737182d6614a4e03926b))
* **guest:** make a parked block and its wire bit one statement ([dba5625](https://github.com/elct9620/kobako/commit/dba5625c541c9b54caa89f5937419ec416ec05a8))
* **guest:** read a String's bytes instead of rendering it ([de8c238](https://github.com/elct9620/kobako/commit/de8c238e900830f72d374f83d8c8e5b63cef4384))
* **guest:** refuse text the capability gems cannot read as text ([539045a](https://github.com/elct9620/kobako/commit/539045a80192cc28c23a4bcbebc311c55eb138fb))
* **payload:** stop claiming a zero-copy decode the args position cannot have ([c56cb9c](https://github.com/elct9620/kobako/commit/c56cb9c30dc727d9ad0510e3f1572c1ab53463d9))
* **release:** drop the version from a dev-dependency nothing rewrites ([524e536](https://github.com/elct9620/kobako/commit/524e5365448fedb101571a11de546dddebbfbcb6))
* **sig:** declare the dispatch seam the shape the ext actually calls ([f0de465](https://github.com/elct9620/kobako/commit/f0de46500eda366d3edd7869edcc39d843f4b687))
* **sig:** drop the Pool method the signature outlived ([35a5b4f](https://github.com/elct9620/kobako/commit/35a5b4ff3e834b157d695819272c2b445f0bb5bf))
* **tasks:** let a retired anchor hold the ceiling it was assigned ([a9cd51d](https://github.com/elct9620/kobako/commit/a9cd51d2bd454698833024c89c3693b0d73ef505))
* **wire:** keep the ABI at 3, which no release has shipped ([2b301b7](https://github.com/elct9620/kobako/commit/2b301b777384c0edf79426805cfb1cc08688aa95))


### Documentation

* **customization:** the harness carries no codec until a shell asks ([a9bf10c](https://github.com/elct9620/kobako/commit/a9bf10c20f27f52183932dd149768626d70f31bf))
* **spec:** separate what a name promises from what the wire promises ([545bfbd](https://github.com/elct9620/kobako/commit/545bfbd59834bab6d564850d9adafa892cbae005))
* **spec:** speak one word for the arm every envelope answers success on ([f4ef5f5](https://github.com/elct9620/kobako/commit/f4ef5f5c096cb2c5b0153f7a28f7feaf15a405d0))
* **wire:** move the Reply's fault arm onto the envelope ([3a56e89](https://github.com/elct9620/kobako/commit/3a56e892caea5b2024baeeeff0bcb8b0169e9b16))


### Code Refactoring

* **gem:** follow Ruby's own convention for what an error is called ([b1ce1fe](https://github.com/elct9620/kobako/commit/b1ce1fe994e5123a950ba9b9f64810387822bb46))
* **guest:** let the shell ask for the codec instead of inheriting it ([abc24d3](https://github.com/elct9620/kobako/commit/abc24d3321c614f36f8c01c932169083796a0915))
* **guest:** name the payload seam for what it is — a codec ([b5d90e2](https://github.com/elct9620/kobako/commit/b5d90e26cdea1e0048cb44e48c6543a6bad4a592))
* **guest:** route a dispatch through kobako-core without reading its payload ([62c5791](https://github.com/elct9620/kobako/commit/62c5791eb88b9021b3d60c8a3dd45d5907011214))
* **mruby:** name each codec method after the position it serves ([e2d4b53](https://github.com/elct9620/kobako/commit/e2d4b53dd2d165d4f6312c8e87a5dd936d2f7a2f))
* **runtime:** give the engine contract names an implementer can write ([0a41491](https://github.com/elct9620/kobako/commit/0a41491360cf17def8245c0758679e92572b990f))
* **sdk:** keep the root to what every build of this crate has ([7f93096](https://github.com/elct9620/kobako/commit/7f93096d4371e7e7f70f3b31301fad3a3198184a))
* **transport:** let every envelope spell its success arm the same way ([df595e0](https://github.com/elct9620/kobako/commit/df595e08097af9e3e0a0c5ed52a4a6f854eb21dc))
* **transport:** name the core envelope's types after what they carry ([b44de65](https://github.com/elct9620/kobako/commit/b44de6502953826c567d4ef8a594561479f50c1d))
* **transport:** split the invocation module along the line its doc draws ([4d3a2d3](https://github.com/elct9620/kobako/commit/4d3a2d3060167d71a807e203688186740fd1485b))
* **wasmtime:** put every cap in one struct, named the way the field is read ([3f57860](https://github.com/elct9620/kobako/commit/3f578608b044d1ade0ae35231266cf3c9e517c02))
* **wire:** give the fault concept one name on both sides of the boundary ([564798a](https://github.com/elct9620/kobako/commit/564798a9662f547f665530a75d99c73051a79d86))
* **wire:** keep a Fault to what its author can bound ([141209d](https://github.com/elct9620/kobako/commit/141209df0f5525853462fddcfaf3584ea530038b))
* **wire:** route every tier through the one envelope ([c5cd33a](https://github.com/elct9620/kobako/commit/c5cd33a5346f49857fa6e1f45c9cf9b9bea0ff77))

## [0.20.0](https://github.com/elct9620/kobako/compare/v0.19.0...v0.20.0) (2026-07-24)


### Features

* **bench:** mark a run in progress with a pid lock file ([3ca85fb](https://github.com/elct9620/kobako/commit/3ca85fbe9860ce32d58c1a5bf4ff85cbdc210780))
* **execution:** tell a nil-value success from a failure with #failed? ([2f3d382](https://github.com/elct9620/kobako/commit/2f3d382b24d65fabda7771429cd3adfc90a8c7e8))
* **extension:** declare a backend's kind by explicit keyword ([3f6c628](https://github.com/elct9620/kobako/commit/3f6c6283080ba4346ac8ef878e469dab8039e650))
* **sandbox:** add the gvl: hold/release scheduling option ([ca2a884](https://github.com/elct9620/kobako/commit/ca2a884415b3271c44b20d131536b3fe7c5a3077))
* **sandbox:** declare fillable Service paths defaulting to Kobako::Unresolved ([1d60f76](https://github.com/elct9620/kobako/commit/1d60f764ddff3b5b10206993e2b281c838ec0022))
* **sandbox:** fill and override declared paths per invocation with ctx.bind ([629ef7d](https://github.com/elct9620/kobako/commit/629ef7dfa35273c952685fefb4c5793b11886aee))
* **sandbox:** return a frozen Kobako::Invocation from #eval and #run ([e9f4634](https://github.com/elct9620/kobako/commit/e9f463403bea26fad32e52d560b979684f555ed1))
* **tasks:** gate the wasmtime driver against a magnus dependency ([268c3f6](https://github.com/elct9620/kobako/commit/268c3f65f53d64b465458bca5c0c9b0d0467df44))


### Bug Fixes

* **context:** raise ArgumentError for a spent ctx.bind ([2783bba](https://github.com/elct9620/kobako/commit/2783bba20f139554178389d4c860e0c280cab550))
* **extension:** re-assert dependencies after a failed seal ([72839ba](https://github.com/elct9620/kobako/commit/72839bac6186e81a1d4323bd3f1eb4f10abf3d64))

## [0.19.0](https://github.com/elct9620/kobako/compare/v0.18.0...v0.19.0) (2026-07-19)


### ⚠ BREAKING CHANGES

* **readme:** guest scripts using `class X < Kobako::Member` must switch to `extend Kobako::Proxy`; Kobako::Member no longer exists in the guest.

### Bug Fixes

* **readme:** switch the Extension File idiom off the removed Kobako::Member ([0d96b9e](https://github.com/elct9620/kobako/commit/0d96b9eafed11df6fa030c38f93b2a0e9b190ed8))

## [0.18.0](https://github.com/elct9620/kobako/compare/v0.17.0...v0.18.0) (2026-07-18)


### Features

* **bench:** add bench:all whole-round sweep ([6163cdf](https://github.com/elct9620/kobako/commit/6163cdffc6aa701a0306af40cc18121787b07e26))


### Bug Fixes

* **guest:** partition dispatch args by Ruby 3 call semantics (B-58) ([59a15a2](https://github.com/elct9620/kobako/commit/59a15a2d524b1ab902bc5e4a2763ffccb485e399))


### Performance Improvements

* **bench:** re-bless the anchor onto the codec-decomposition round ([74dca53](https://github.com/elct9620/kobako/commit/74dca5374a587242f8b1e00768dd33c330ff298c))

## [0.17.0](https://github.com/elct9620/kobako/compare/v0.16.0...v0.17.0) (2026-07-17)


### Features

* **examples:** add vfs, an overlay filesystem that protects the disk ([1c83d44](https://github.com/elct9620/kobako/commit/1c83d44de971bf76d3db84cc3021b23bfc363842))
* **tasks:** add a rake gate aggregate over every gate:* check ([c02e069](https://github.com/elct9620/kobako/commit/c02e06996134f124025538ba5c23f923606bc3d2))
* **tasks:** add KobakoReport, the shared static-analysis output template ([7854a61](https://github.com/elct9620/kobako/commit/7854a61d30b823534bcad869a4e4970beab80340))
* **tasks:** disclose the Ruby-only scope of the coverage report ([51cb291](https://github.com/elct9620/kobako/commit/51cb291bbc45671240a63e00a1461958694e4ee1))
* **tasks:** disclose the tail below the hotspots top-N cut ([d7584ee](https://github.com/elct9620/kobako/commit/d7584ee610e347dcef9cb2515680c97c911320e5))
* **tasks:** gate the pub-surface acknowledgement ledger for staleness ([5f97c77](https://github.com/elct9620/kobako/commit/5f97c773f300363e387ff8020523f141cdaaf88f))
* **tasks:** gate the RBS collection lock against Gemfile.lock drift ([8201ce0](https://github.com/elct9620/kobako/commit/8201ce03848d9710239c586321a6ffb75de1950d))
* **tasks:** measure Rust line coverage with cargo llvm-cov ([1f6bd9f](https://github.com/elct9620/kobako/commit/1f6bd9f2876f3ef6d8e60880c40198eb70327bf4))
* **tasks:** open the stats reports with a self-describing banner ([914d0cf](https://github.com/elct9620/kobako/commit/914d0cf74e338a0a7405cc6f981135defac534f1))


### Bug Fixes

* **catalog:** match Extension dependencies by Symbol form ([57f17b8](https://github.com/elct9620/kobako/commit/57f17b872e506f3935ebd4ca1f9d81c4bc34455f))
* **codec:** cap the host wrap-walk nesting depth (E-54) ([be67f37](https://github.com/elct9620/kobako/commit/be67f37108f24699354950f4e72293169da66917))
* **codec:** keep the guard id out of the wire-symmetry inventory ([e51998b](https://github.com/elct9620/kobako/commit/e51998bc7d54715aecb58f3b7817c337cd882c8b))
* **codec:** reject a non-representable #run argument Hash key cleanly ([dbabace](https://github.com/elct9620/kobako/commit/dbabaceecb6f32f37a26839b8162c0e94fe881bb))
* **codec:** reject non-wire values via a factory guard, not to_msgpack ([8e5c6b8](https://github.com/elct9620/kobako/commit/8e5c6b8cb67faa2eb7b82d6e63e3f8464b1126a0))
* **codec:** tighten map-decode pre-allocation to the true pair bound ([7094f91](https://github.com/elct9620/kobako/commit/7094f91898dae4c91c4e6863b09d25a6f34d096e))
* **examples:** contain overlay writes and prove the vfs traversal guard ([2fe7965](https://github.com/elct9620/kobako/commit/2fe7965b8f588b160a907771a14e51f80cd8d3a7))
* **examples:** contain vfs reads against symlink escape ([43317c8](https://github.com/elct9620/kobako/commit/43317c82c4a4171c08fdbf0c9fe7014cc249b926))
* **guest:** reject non-representable dispatch args instead of to_s (E-55) ([6b9ab56](https://github.com/elct9620/kobako/commit/6b9ab562753407cf6168a5baad34140afd75a86a))
* **tasks:** land the gate move for parity and wire:symmetry ([a0dddbc](https://github.com/elct9620/kobako/commit/a0dddbca9638418dbf3cd08e29806e05c5198cd3))
* **tasks:** resync the pub-surface ledger to the renamed install cluster ([c6a6c33](https://github.com/elct9620/kobako/commit/c6a6c33f5802f4536a4ebcc2df36acc130059d89))

## [0.16.0](https://github.com/elct9620/kobako/compare/v0.15.0...v0.16.0) (2026-07-12)


### Features

* **sandbox:** add the Extension install mechanism ([ad8e4da](https://github.com/elct9620/kobako/commit/ad8e4da703e4ceee85959663d77bb1e26c26791a))
* **sdk:** add the Extension install mechanism to the Rust host SDK ([4043f76](https://github.com/elct9620/kobako/commit/4043f764b7038619a30c16542ed38d566e4a72a9))
* **tasks:** break each module down by language in stats:&lt;slug&gt; ([b496e3c](https://github.com/elct9620/kobako/commit/b496e3cb23150f8f073cc1b972978cfbebfbb1e6))
* **tasks:** report code sizes per publishable module in stats:all ([f554930](https://github.com/elct9620/kobako/commit/f554930e669a82bed0c7305c5cbbab96c3ae79da))
* **tasks:** split impl vs inline test per module in stats:all ([cefa38d](https://github.com/elct9620/kobako/commit/cefa38d96ff79509b947269ea5c361ead0610bfa))


### Bug Fixes

* **tasks:** count Rust inline tests in the stats code-to-test ratio ([134c970](https://github.com/elct9620/kobako/commit/134c9704a5a9bb6fb01c1aa7968f907d1436e5f2))

## [0.15.0](https://github.com/elct9620/kobako/compare/v0.14.0...v0.15.0) (2026-07-11)


### Features

* **claude:** cover rake files in the post-edit rubocop hook ([416cd42](https://github.com/elct9620/kobako/commit/416cd42009e7888fb0662e91719f4d7d61713cf8))
* **examples:** add plugin-rs, a Rust SDK narrative example ([8263096](https://github.com/elct9620/kobako/commit/8263096876900aa2c3ffeebbd24f7cca3571b893))
* **examples:** add wire-rs, a low-level host without the SDK ([d9edea8](https://github.com/elct9620/kobako/commit/d9edea8ed025e4b7ffe58be8c376c7fb0a65ac04))
* **sandbox:** flatten Service registration to path-valued bind ([0876006](https://github.com/elct9620/kobako/commit/0876006455544fd82eb7555ee80c149d98843719))
* **tasks:** add grouped test:tasks / test:bench runs ([1153c43](https://github.com/elct9620/kobako/commit/1153c43416363d7cc03b696eb2762b7281a68e93))
* **tasks:** compare the wire-symmetric peer inventories mechanically ([50d4fe5](https://github.com/elct9620/kobako/commit/50d4fe523bf85406931fa9611b6528c3de1a3f50))
* **tasks:** flag stale ledger entries in the surface and symmetry instruments ([d18054f](https://github.com/elct9620/kobako/commit/d18054f5daa6ce277154c288d5c3085133f441b4))
* **tasks:** hold the pub-surface crate map to the repo's crate roster ([4f65e79](https://github.com/elct9620/kobako/commit/4f65e79c3aa37ed1fc47a5002c8a567c3df8cc84))
* **tasks:** hold the tier roster to the repo and share it across instruments ([9a75586](https://github.com/elct9620/kobako/commit/9a755866236c134ea7601006211338eefae0a348))
* **tasks:** inventory exported macros in the pub-surface scan ([e4457ea](https://github.com/elct9620/kobako/commit/e4457eafecfa24eb5fc9aade086f2be9b0862818))
* **tasks:** measure the tooling tiers in stats and hotspots ([8def9ff](https://github.com/elct9620/kobako/commit/8def9ff96d36c2020e04d7e9bb0d9cd3fdad3e43))
* **tasks:** pin the tier roster to the repo from both sides ([3b7a154](https://github.com/elct9620/kobako/commit/3b7a154ebbfe8629392a81599df37d136a0eec99))
* **tasks:** rank churn x size x fan-in hotspots since the last release ([73b3bc8](https://github.com/elct9620/kobako/commit/73b3bc8fdee19f8c09a91ccf96348688ea3ccd1c))
* **tasks:** read the anchor citation profile back as an instrument ([47961cb](https://github.com/elct9620/kobako/commit/47961cb689d9c681f5c697af7e6bca52a4c7817f))
* **tasks:** report pub items no in-repo downstream consumes ([3eb0481](https://github.com/elct9620/kobako/commit/3eb048199a0e2e41509513750bec4fcbc6b7cc69))
* **tasks:** resolve the SPEC-local F/J/N families in the anchors gate ([33d3118](https://github.com/elct9620/kobako/commit/33d31181be3a800a4cc3d4eb32324d840473401f))
* **tasks:** scan pub statics in the pub-surface instrument ([72cff5e](https://github.com/elct9620/kobako/commit/72cff5ef369a73c353ecdd25c1d4688ec8dd2f82))
* **tasks:** score hotspots on impl-only lines for Rust files ([704b419](https://github.com/elct9620/kobako/commit/704b41971eac9e284b570bd0ac2fdd9af656bbca))
* **tasks:** seat anchors:coverage and wire:symmetry in the release gate ([ab035bb](https://github.com/elct9620/kobako/commit/ab035bb3c4e049e6319d33cdaf1d715d4e83eedb))
* **tasks:** widen the fan-in and ext-code scans to their whole tiers ([43d2bad](https://github.com/elct9620/kobako/commit/43d2badfea3c13cb062e67d5ded8410a339862d3))


### Bug Fixes

* **codec:** reject ext 0x02 anywhere in the Panic frame, not only details ([062e29d](https://github.com/elct9620/kobako/commit/062e29d6ee15264e1bd942502b751cfe7610acad))
* **codec:** reject the Fault envelope in Rust host payload positions ([bdf2ed7](https://github.com/elct9620/kobako/commit/bdf2ed78fde2798bdc15f4e969bda228cf482f4b))
* **codec:** reject the reserved Handle id 0 on the Rust wire tier ([5f7e482](https://github.com/elct9620/kobako/commit/5f7e4821680e553da355d5257b0619e4a1cdce72))
* **tasks:** fail the anchors gate when no SPEC ceiling parses ([8d185f5](https://github.com/elct9620/kobako/commit/8d185f59afc28b5334d934f6916fd64e458e9359))
* **tasks:** inventory every codec-bearing class per transport file ([e553ae1](https://github.com/elct9620/kobako/commit/e553ae1578245d9bfca63cf4e8429666857dbb92))
* **tasks:** keep tooling-suite fixture tokens out of the anchor scans ([58a78b9](https://github.com/elct9620/kobako/commit/58a78b990684d4ad87d4db18b21eb8bd1c98713a))
* **tasks:** mark unmeasured fan-in as '-' in the hotspot report ([a4959fe](https://github.com/elct9620/kobako/commit/a4959fe0c93546711dd16002e38d993e350bcb8f))
* **tasks:** read pub fn qualifiers in the pub-surface scan ([ac0e7d3](https://github.com/elct9620/kobako/commit/ac0e7d3030729ad981b6a8de504c5837daf2038d))
* **tasks:** read the codec-bearing class in the wire-symmetry inventory ([85ce937](https://github.com/elct9620/kobako/commit/85ce937fdb35953b26fee5fa4ee55f53988147ea))
* **tasks:** scan the whole tier in every wire-symmetry inventory ([c2bba5c](https://github.com/elct9620/kobako/commit/c2bba5c02ca20dca14d061a03f8f4581d0995a66))
* **tasks:** truncate the pub-surface scan only at the test module ([9c6dc05](https://github.com/elct9620/kobako/commit/9c6dc054bfa1fe79c3410fa794290a1ce5b85f4d))
* **test:** flunk instead of skip when CI lacks a built guest prerequisite ([f7818f6](https://github.com/elct9620/kobako/commit/f7818f68c8114e5f9b0012a4dd4b069695bc3e32))
* **transport:** refuse the Fault envelope outside its legal position (E-50) ([58c5069](https://github.com/elct9620/kobako/commit/58c50691d32e245f41c2f8041dfb7e92b05b80e9))


### Performance Improvements

* **bench:** re-bless the anchor onto the wasmtime 46 round ([9225702](https://github.com/elct9620/kobako/commit/9225702106c0c4caf90425ac08c11452b6388b55))
* **dispatch:** skip the Handle walk when the request carried none ([cd63514](https://github.com/elct9620/kobako/commit/cd6351459c492084a401fd1b6a8e4293761de93f))
* **sandbox:** skip the Handle walk when the invocation result carried none ([d2c4947](https://github.com/elct9620/kobako/commit/d2c494782d316df3bbf5fc223a8beaa1f04f5718))
* **transport:** skip the Handle walk when the yield result carried none ([bca463b](https://github.com/elct9620/kobako/commit/bca463bce0b8913941b3e3ba11c31cb41b843545))

## [0.14.0](https://github.com/elct9620/kobako/compare/v0.13.0...v0.14.0) (2026-07-08)


### Features

* **codec:** add the Run invocation envelope to the wire tier ([dbdd760](https://github.com/elct9620/kobako/commit/dbdd760ddc258681669f7f620f63b75f36322687))
* **crates:** add the kobako host SDK skeleton ([8a99d09](https://github.com/elct9620/kobako/commit/8a99d09ef7068a6738d44f1a735d39516b24156b))
* **crates:** add the parity runner to the kobako SDK ([998f059](https://github.com/elct9620/kobako/commit/998f059abd308ef921c295658aaf8377febb44e2))
* **crates:** grow the SDK capability-Handle table ([f93fe8f](https://github.com/elct9620/kobako/commit/f93fe8f3dce2509cfa527229f8f593f4d816b940))
* **crates:** grow the SDK Member block-yield seam ([4404713](https://github.com/elct9620/kobako/commit/44047130f309a2c935198077fb4f7f86839355e7))
* **crates:** grow the SDK preload and run invocation seams ([d8d5fe2](https://github.com/elct9620/kobako/commit/d8d5fe268a56d45dad4f8b35a25e942a559dcd5f))
* **crates:** honor the respond_to_guest narrowing on the SDK Member seam ([0f5eff1](https://github.com/elct9620/kobako/commit/0f5eff16a1e9f2229ad9d9c9316bf94e92035301))
* **crates:** let a resolved Handle recover its concrete member type ([abd5502](https://github.com/elct9620/kobako/commit/abd55029a44a7631d323c3aec3b625d9692f9c5b))
* **crates:** mark the SDK Error taxonomy non_exhaustive ([001fc69](https://github.com/elct9620/kobako/commit/001fc69e637d2c046f55cb517f9d9cf931793715))
* **examples:** add the guest→host dispatch half to the Rust host ([5cdde68](https://github.com/elct9620/kobako/commit/5cdde68824352c5685aaa94ae464e909705a4648))
* **examples:** assemble a minimal Rust host on the published crates ([6b98680](https://github.com/elct9620/kobako/commit/6b98680bcb625f9790f44df839035427a576e266))
* **tasks:** add a rails-stats-style rake stats size report ([c02d530](https://github.com/elct9620/kobako/commit/c02d530d43a85594323b7801fdbe75aa95ae7f8c))
* **tasks:** gate parity coverage over the CORE anchor manifest ([d26972c](https://github.com/elct9620/kobako/commit/d26972c1ad51e05df951643e17d66a6c811ba862))


### Bug Fixes

* **crates:** give SetupError a Display so Error::Setup reads cleanly ([bc8b128](https://github.com/elct9620/kobako/commit/bc8b128c9e296319c7fef47441b412d2ce345dff))
* **crates:** keep the no-timeout epoch deadline within range ([a3255df](https://github.com/elct9620/kobako/commit/a3255df98a77825bb39b571a60fe6ff83d269d19))
* **crates:** reject trailing bytes on Request and Run decode ([8e4929b](https://github.com/elct9620/kobako/commit/8e4929b64f3ea690f38211888070c2511da84754))
* **lib:** close the Data #with seam on Handle construction ([c0b50ae](https://github.com/elct9620/kobako/commit/c0b50aec659e32a72c905aaf94c939ac009ad621))
* **release:** resume the gem push loop past already-live versions ([170ee9b](https://github.com/elct9620/kobako/commit/170ee9bae6da3a65b4016040259e31e4721dfcfb))
* **release:** treat a yanked-only version as unpublished when publishing ([3111cc0](https://github.com/elct9620/kobako/commit/3111cc072a7c9d18b129fd2762f267d855b48d55))
* **transport:** fold fault messages to UTF-8 before they ride the wire ([0be8d40](https://github.com/elct9620/kobako/commit/0be8d4068dc5647f86746e80b356b0b9d52051f7))

## [0.13.0](https://github.com/elct9620/kobako/compare/v0.12.2...v0.13.0) (2026-07-03)


### Features

* **crates:** build the requested isolation profile into the WASI context ([63c25d8](https://github.com/elct9620/kobako/commit/63c25d835d4d03010c1658217cee412318e6b5d8))
* enforce the isolation-profile floor at Sandbox construction ([73f0dfe](https://github.com/elct9620/kobako/commit/73f0dfe34a144045a559a28caec31468351de64a))
* forward the Sandbox profile request through the ext to the driver ([f47f906](https://github.com/elct9620/kobako/commit/f47f9063aa56462be023f5c76cbc73db6e4315ad))
* keep guest output readable after a trap ([464454c](https://github.com/elct9620/kobako/commit/464454cd774db96ad72b3787007f2ce015673587))
* **runtime:** runtimes declare their isolation profile ([f89717a](https://github.com/elct9620/kobako/commit/f89717a6bc9809f0de0df78f97da33a49a1474ac))

## [0.12.2](https://github.com/elct9620/kobako/compare/v0.12.1...v0.12.2) (2026-07-02)


### Bug Fixes

* **codec:** bound ext-envelope nesting to keep deep Fault chains off the native stack ([7bed2b2](https://github.com/elct9620/kobako/commit/7bed2b2f43a63538aa60610c82d2eb65bcce7b15))
* **guest:** size collection conversions by C array length, not #length ([90ecbd0](https://github.com/elct9620/kobako/commit/90ecbd0cb6a990b8c5a1e5deec3a10df4eaa37df))
* **io:** enforce the fd allowlist at the write syscall ([1b300df](https://github.com/elct9620/kobako/commit/1b300df7bee8f87b701f76b42300163a8899b93e))
* **release:** advance last-release-sha past the unparseable fork merge ([d06117d](https://github.com/elct9620/kobako/commit/d06117d462eea0ae5648e5bdd6886b735765f3b3))
* **sandbox:** honor nil to disable the output caps, and validate them ([51d1e90](https://github.com/elct9620/kobako/commit/51d1e900e3d7d659ffd432ff3d613786e9073b05))

## [0.12.1](https://github.com/elct9620/kobako/compare/v0.12.0...v0.12.1) (2026-06-27)


### Bug Fixes

* **spec:** resolve nested Handle dispatch arguments symmetric with B-37 ([bb4ca9a](https://github.com/elct9620/kobako/commit/bb4ca9a0fef0f17a6e97a63a3ee4a3784961fbe2))
* **transport:** resolve nested Handle dispatch arguments to host objects ([de5b233](https://github.com/elct9620/kobako/commit/de5b233a2750684ce1c6cceb3d718da1194eabc1))

## [0.12.0](https://github.com/elct9620/kobako/compare/v0.11.2...v0.12.0) (2026-06-26)


### Features

* **bench:** inject the Guest Binary into probes via KOBAKO_BENCH_WASM ([3408058](https://github.com/elct9620/kobako/commit/3408058f5a4319725863dd3bd1b305cc4609bbf8))
* **kobako-json:** classify generate values by native mruby type ([f8e5de3](https://github.com/elct9620/kobako/commit/f8e5de3a2bdc0234290101734f2cceff88058cd6))
* **spec:** specify the guest JSON capability (B-52/B-53, docs/json.md) ([b666d6f](https://github.com/elct9620/kobako/commit/b666d6fecc230eef0b01ae34cdb9eab2bbc6b153))
* **wasm:** compose the json and full Guest Binary variants ([002b16d](https://github.com/elct9620/kobako/commit/002b16d88ea7c9dbf0493185bbac757b782bc28d))


### Bug Fixes

* **bench:** correct results/baseline path depth after support dir move ([faf1d8a](https://github.com/elct9620/kobako/commit/faf1d8aaf185159edced2d421696c721da9ed15c))
* **spec:** classify Symbol as directly-encoded in JSON generate ([10a8671](https://github.com/elct9620/kobako/commit/10a867145ed8ccb0b0e2d8e61c0ae84ae0ef3399))
* **spec:** close the JSON Hash-key host-dispatch gap and refresh the anchor range ([aea1313](https://github.com/elct9620/kobako/commit/aea13133efc957c70f8ff3f77fd2164c634ebb86))
* **spec:** complete the JSON generate Hash-key partition and tighten wording ([b01a3f2](https://github.com/elct9620/kobako/commit/b01a3f26baf2e8364ff82a9d292b426444e0acd7))
* **spec:** keep B-52 from re-enumerating the JSON-carrying variants ([13a5723](https://github.com/elct9620/kobako/commit/13a57236e1b4b33c5e84caa8bb940a0b8560a6d0))
* **spec:** let pretty_generate own its layout instead of pinning CRuby ([bf78bf1](https://github.com/elct9620/kobako/commit/bf78bf1a8ab8e125f35eacf6e2151ec94f87d46b))
* **spec:** make the JSON depth bound consistent across parse and generate ([efdc535](https://github.com/elct9620/kobako/commit/efdc535f5e70f67efb8638abcd4b7b1881cedf35))
* **spec:** make variant pointers source-free and pin generate edge cases ([8aeeecd](https://github.com/elct9620/kobako/commit/8aeeecd674ba24f4caab3cc3ea4e905b182061b0))
* **spec:** note the full variant in regexp.md availability ([7e50f27](https://github.com/elct9620/kobako/commit/7e50f27a593ed8d8582f24961c8725fc252699b5))

## [0.11.2](https://github.com/elct9620/kobako/compare/v0.11.1...v0.11.2) (2026-06-24)


### Bug Fixes

* **codec:** encode guest Handle args/kwargs as ext 0x01 ([bd58538](https://github.com/elct9620/kobako/commit/bd58538f4dbbf91a0927d15fd37f47abc761f8a6))
* **codec:** refuse out-of-range inbound integers instead of saturating ([f9e9184](https://github.com/elct9620/kobako/commit/f9e91845e0f28fecbb0867d8b70c871cd1feafea))
* **dispatch:** keep short method names intact across kwarg unpacking ([c6e4a6f](https://github.com/elct9620/kobako/commit/c6e4a6f268970c0c2d2851d3a23e3bec153dc56d))
* **release:** drop the group PR title pattern that breaks tagging ([d797350](https://github.com/elct9620/kobako/commit/d797350953c2de82df592d3c88e44155d3f076dc))
* **release:** scope publish triggers to each release's owning tag ([58508a1](https://github.com/elct9620/kobako/commit/58508a1e967799522b2674a989233d8619cabbc9))

## [0.11.1](https://github.com/elct9620/kobako/compare/v0.11.0...v0.11.1) (2026-06-14)


### Bug Fixes

* **guest:** adopt beni 0.7.0 protected dispatch (B-51) ([c61655b](https://github.com/elct9620/kobako/commit/c61655bcead336d32a4b6ff7ff1b34c21cdfccd9))

## [0.11.0](https://github.com/elct9620/kobako/compare/v0.10.0...v0.11.0) (2026-06-13)


### Features

* **transport:** narrow guest-reachable methods via respond_to_guest? ([7a25fe3](https://github.com/elct9620/kobako/commit/7a25fe3b440b523b8a692b7601d08877b1305b0d))

## [0.10.0](https://github.com/elct9620/kobako/compare/v0.9.2...v0.10.0) (2026-06-12)


### Features

* **catalog:** reject member binding after the seal (E-45) ([5193ed6](https://github.com/elct9620/kobako/commit/5193ed64100fa8ac05a2ba18cfa00634b4f40e6f))
* **guest:** bake the canonical boot state and instantiate per invocation (B-49) ([ee9ae6e](https://github.com/elct9620/kobako/commit/ee9ae6e09eab30f54dba0eeec00a5a2c80da819f))
* **pool:** add Kobako::Pool warm-Sandbox checkout (B-46..B-48) ([abf9bf8](https://github.com/elct9620/kobako/commit/abf9bf8d3c725c0ca0b8f2ab8b2ddd6f71ee6de4))


### Bug Fixes

* **ext:** give the ABI probe a WASI context ([18e21ea](https://github.com/elct9620/kobako/commit/18e21eac8b160ade2724578aeacd86170403ee2c))
* **ext:** trust the artifact disk cache only in an exclusively writable directory ([17679cc](https://github.com/elct9620/kobako/commit/17679cc0d38d2a1b605e2faaeed762477a718c18))


### Performance Improvements

* **bench:** re-bless the anchor onto the post-0.9.2 performance round ([195224d](https://github.com/elct9620/kobako/commit/195224d5d48e53dc2be17752e6d6af6382a0c1ec))
* **catalog:** drop the alloc-path block iteration from the gadget refusal ([542fe59](https://github.com/elct9620/kobako/commit/542fe59464bde57283d8e91984b82e82592bc3ab))
* **ext:** amortise module compilation across processes via .cwasm cache ([2e688bc](https://github.com/elct9620/kobako/commit/2e688bc4a1cdf1d0d4d5a0bce2efb314a5b8d1f7))
* **ext:** bound and harden the compiled-artifact cache ([949f222](https://github.com/elct9620/kobako/commit/949f2227af7cdf7d1913dcae58df683912a7dbd5))
* **ext:** cache ABI export handles and per-path InstancePre ([47573d0](https://github.com/elct9620/kobako/commit/47573d022233c788ce94413d1a2901ee9d62fc2e))
* **lib:** cache sealed frame encodings and cut decode-walk allocations ([e599573](https://github.com/elct9620/kobako/commit/e599573e37531b363baca83a1aa5833930100320))

## [0.9.2](https://github.com/elct9620/kobako/compare/v0.9.1...v0.9.2) (2026-06-11)


### Bug Fixes

* **catalog:** never mint a Capability Handle for a reflective gadget ([6c2d29d](https://github.com/elct9620/kobako/commit/6c2d29d0fbcced5187df5538c2c6c437705fd6d8))
* **ext:** cap stdin frames at 16 MiB like the run envelope ([a94099a](https://github.com/elct9620/kobako/commit/a94099a03830929c55fcb266e227073db9c5a624))
* **ext:** deny guest ambient clock and entropy at the WASI layer ([1275b35](https://github.com/elct9620/kobako/commit/1275b35264813628a2aeb396a3546faf1f6d9d0c))
* **transport:** reject reflective gadget methods in guest dispatch ([948fb9e](https://github.com/elct9620/kobako/commit/948fb9ea7d6c0d6bd91f6e261d3263743974388b))
* **wasm:** mirror the reflection rejection in the guest proxy ([f6ead3b](https://github.com/elct9620/kobako/commit/f6ead3b91f1ac92c3c075397d177edb4b82cd15d))

## [0.9.1](https://github.com/elct9620/kobako/compare/v0.9.0...v0.9.1) (2026-06-11)


### Bug Fixes

* **transport:** block ambient reflection in guest dispatch (GHSA-7pwq-q9jf-539h) ([dd08166](https://github.com/elct9620/kobako/commit/dd081665f368f7ba54e476c3ad045ee1aa8ed703))

## [0.9.0](https://github.com/elct9620/kobako/compare/v0.8.0...v0.9.0) (2026-06-10)


### Features

* **regexp:** add Kernel#=~ fallback returning nil ([d461781](https://github.com/elct9620/kobako/commit/d4617815b8e888a09a548c7f4664819cdddc34c8))
* **regexp:** add regexp-aware String#[]= ([34807f5](https://github.com/elct9620/kobako/commit/34807f5ba5f6e17efff27af3c5c24ae42b0b651d))
* **regexp:** add Regexp.last_match and last_match= ([03649e8](https://github.com/elct9620/kobako/commit/03649e81cfda0c43ac22777f70ea38a0ac4a93c7))
* **regexp:** add Regexp#named_captures and #names ([7cf018d](https://github.com/elct9620/kobako/commit/7cf018d39529be1d1384297b1b38b9d1670523e7))
* **regexp:** add String#slice! ([2857e0e](https://github.com/elct9620/kobako/commit/2857e0ee55df9f6295a25391ff76613b8dd5d555))
* **regexp:** align Regexp#match position handling with MRI ([c448c94](https://github.com/elct9620/kobako/commit/c448c9402a5a78076b5a81be0e07b7e1c90b1014))
* **regexp:** align Regexp#to_s flag rendering with MRI ([67d0414](https://github.com/elct9620/kobako/commit/67d04145f116a72a0f84d0ddf6674559e97046e8))
* **regexp:** copy the compiled pattern on Regexp dup/clone ([be97ea1](https://github.com/elct9620/kobako/commit/be97ea1cbd9556196f06229cd17d0288c062f133))
* **regexp:** copy the match snapshot on MatchData dup/clone ([0719d99](https://github.com/elct9620/kobako/commit/0719d99c41df9ddc8905580d61b32f0e6d88b6ba))
* **regexp:** define RegexpError in the gem instead of borrowing it ([ca57ca6](https://github.com/elct9620/kobako/commit/ca57ca6effb93ad05a175f2641f4b15aa971e31c))
* **regexp:** escape the source in Regexp#inspect ([9142d6c](https://github.com/elct9620/kobako/commit/9142d6cd32e08271639548af7801284e5d198892))
* **regexp:** expand backreferences and Hash in gsub/sub replacements ([8a0bc2d](https://github.com/elct9620/kobako/commit/8a0bc2dda8fb46a91fb1a5ee1c7482d6da9dffee))
* **regexp:** forbid MatchData.new ([5e2b3f5](https://github.com/elct9620/kobako/commit/5e2b3f527ff68dd118c370bb1b0bd01bf3dc4f8f))
* **regexp:** honour MatchData#named_captures(symbolize_names:) ([2a754d3](https://github.com/elct9620/kobako/commit/2a754d3d2ca3d4159471c8f9d17cc71ac59e0543))
* **regexp:** honour the position argument in String#index ([4dfbb41](https://github.com/elct9620/kobako/commit/4dfbb41086b302eada56efea4cbbfd6579adbdab))
* **regexp:** memoize compiled patterns per invocation ([f764d66](https://github.com/elct9620/kobako/commit/f764d66a574da51b9e714db1ae6d917cce4cf611))
* **regexp:** raise IndexError for out-of-range MatchData#begin/#end/#offset ([85fc8d6](https://github.com/elct9620/kobako/commit/85fc8d67d0fcc137a7f43695ead34317d557ec1a))
* **regexp:** reproduce the C match-family operand handling ([9e30d2d](https://github.com/elct9620/kobako/commit/9e30d2d6d6ddd42c84d5e2fb55cc2c06076c4fd4))
* **regexp:** set the $+ last-group match global ([b65a424](https://github.com/elct9620/kobako/commit/b65a42434b2edd5b01ed879b09895d17ff778888))
* **regexp:** support length and Range forms of MatchData#[] ([7ac4dee](https://github.com/elct9620/kobako/commit/7ac4deec79a215b35fdc2786522145ce6d34263c))
* **regexp:** yield the MatchData to a block in Regexp#match / String#match ([f5a6e53](https://github.com/elct9620/kobako/commit/f5a6e53ec5993237805c2360fa70684f84acf6b8))
* **wasm:** split the Guest Binary into a pure default and regexp variants ([21695b1](https://github.com/elct9620/kobako/commit/21695b1cd527e334c68f9730f01f92fef057e52f))


### Bug Fixes

* **regexp:** align String#=~ with MRI semantics ([c8f3e70](https://github.com/elct9620/kobako/commit/c8f3e70c9f0797c614aab1639934d280fe20b90a))
* **regexp:** bound backtracking, clamp match positions, harden engine errors ([0177f71](https://github.com/elct9620/kobako/commit/0177f71cccf61c140912aab3c2e639286fd768d0))
* **regexp:** correct String#split group and zero-width handling ([c0150bc](https://github.com/elct9620/kobako/commit/c0150bc04c5dccdd430b173219c42c8f607112d1))
* **regexp:** honour capturing groups and the limit arg in String#split ([66e7398](https://github.com/elct9620/kobako/commit/66e73984f8318b582cc7aa0db48deacfb10e8671))
* **regexp:** make Regexp.last_match= refresh the numbered globals ([e47b257](https://github.com/elct9620/kobako/commit/e47b257715b73263ae5d1f9bae67442197330ee0))
* **regexp:** name the pattern in match-time errors and snap String#index pos ([5835f42](https://github.com/elct9620/kobako/commit/5835f42ae6b27e2a2a0d0bc5bf8023024a20642a))
* **regexp:** stop escaping the slash in Regexp.escape ([6c6f17a](https://github.com/elct9620/kobako/commit/6c6f17a63dbddf30ccdba19ddf3c9b7fbb7772cd))

## [0.8.0](https://github.com/elct9620/kobako/compare/v0.7.0...v0.8.0) (2026-06-05)


### Features

* validate the Guest Binary ABI version at Sandbox construction ([63f22de](https://github.com/elct9620/kobako/commit/63f22deb88dc8acfeae56dccdbf31a7b3650da0d))
* **wasm:** turn the Guest ABI into a trait + export_guest! macro ([3532dc2](https://github.com/elct9620/kobako/commit/3532dc20521ca8d9dd55bc39f01ff611d9df0d4b))


### Bug Fixes

* **build:** track every wasm workspace member in the rebuild check ([3f042a4](https://github.com/elct9620/kobako/commit/3f042a4fe28aa8d8b70bb313f6158e1865a2cbeb))

## [0.7.0](https://github.com/elct9620/kobako/compare/v0.6.2...v0.7.0) (2026-06-03)


### Features

* **examples/async-io:** demo single-thread I/O overlap across Sandboxes ([858a0f7](https://github.com/elct9620/kobako/commit/858a0f70bb0730e5e3ad3f49a5caadf948f6ed7d))
* **guest:** reject construction of Handle proxies (B-39) ([bda5e2b](https://github.com/elct9620/kobako/commit/bda5e2b5e48fe25adeefac19c47f1b93585091bf))
* **guest:** reject construction of Member proxies (B-38) ([885c281](https://github.com/elct9620/kobako/commit/885c2812da8627e4ec7c185d9e04e4056c94a7fd))


### Bug Fixes

* **ext:** surface the buried root cause on non-cap traps ([dbd1ce5](https://github.com/elct9620/kobako/commit/dbd1ce51f25ab8c83d39daee64278192d763d5ef))
* **guest:** bound the encoder walk so cycles and deep nesting fail cleanly ([90880ff](https://github.com/elct9620/kobako/commit/90880ffe6f0ee911b3c7c076ef6c86b9e08c62e1))
* **guest:** stop an embedded NUL in a returned value from hard-trapping ([14fbb97](https://github.com/elct9620/kobako/commit/14fbb97ecb47b4263585602754e92237bb951d46))
* **guest:** stop named-capture regexes from hard-trapping the sandbox ([a279ea1](https://github.com/elct9620/kobako/commit/a279ea1e2f196580b396342851e4e75ff9ea5cfa))

## [0.6.2](https://github.com/elct9620/kobako/compare/v0.6.1...v0.6.2) (2026-05-31)


### Bug Fixes

* **guest:** enable mruby-sprintf for printf and String#% ([1179227](https://github.com/elct9620/kobako/commit/1179227b85b861bccc82d2a258481769a13ed4d5))

## [0.6.1](https://github.com/elct9620/kobako/compare/v0.6.0...v0.6.1) (2026-05-28)


### Bug Fixes

* **loader:** try Ruby-ABI subdir before bare path ([51017eb](https://github.com/elct9620/kobako/commit/51017eb5ee40fa722ec962ce3b9a5d016b128d41))

## [0.6.0](https://github.com/elct9620/kobako/compare/v0.5.0...v0.6.0) (2026-05-28)


### Features

* **bench:** gate against a committed anchor baseline ([ed8b30e](https://github.com/elct9620/kobako/commit/ed8b30e0940736cbabcca18227590d07c3bf94d3))
* **handle:** restore guest-returned Capability Handles to host objects (B-37) ([092815d](https://github.com/elct9620/kobako/commit/092815d610d3595db82b406d4b67880c84f11900))


### Bug Fixes

* **bench:** harden the gate guards and split judgment from the runner ([d6eaae2](https://github.com/elct9620/kobako/commit/d6eaae2de44d14a4735fbb544da712c659144a86))
* **ci:** chain release.yml from release-please via workflow_call ([711665d](https://github.com/elct9620/kobako/commit/711665d29a8c8445b1e26ca08e4b0efc5b24982c))
* **handle:** don't restore a Handle broken out of a guest block (B-37) ([ea25ab9](https://github.com/elct9620/kobako/commit/ea25ab9793f376f15e8d668077ad58f8d67e5a63))

## [0.5.0](https://github.com/elct9620/kobako/compare/v0.4.0...v0.5.0) (2026-05-27)


### Features

* **abi:** add `__kobako_yield_to_block` skeleton + host re-entry channel ([555eb4b](https://github.com/elct9620/kobako/commit/555eb4bf578c3c4397ba2c0d105c0d3ca687e23c))
* **abi:** classify RBreak via ci_break_index for B-25 / E-21 ([32668a0](https://github.com/elct9620/kobako/commit/32668a033e2f959700acadadfbc41388ed72a2dd))
* **abi:** wire `__kobako_yield_to_block` to real `mrb_yield_argv` ([35aeac8](https://github.com/elct9620/kobako/commit/35aeac8700254d1500f5be837a72c56984a7ebfa))
* **bench:** add noise-aware release gate, report mean alongside median ([0cfaebc](https://github.com/elct9620/kobako/commit/0cfaebc2afadfae81e3d00441273da70e396d7a5))
* **bench:** add yield round-trip suite as gated benchmark [#6](https://github.com/elct9620/kobako/issues/6) ([315f923](https://github.com/elct9620/kobako/commit/315f923caa89bcd8752a611525da68ae53ae092f))
* **catalog:** introduce empty Kobako::Catalog namespace ([8af8c54](https://github.com/elct9620/kobako/commit/8af8c54c72e5e5193555bcc2e86072d4a4d8176d))
* **ext:** enforce the 16 MiB single-dispatch payload cap on host boundaries ([c80e281](https://github.com/elct9620/kobako/commit/c80e281e0810640c60d93174beddd49a31c34182))
* **guest:** capture guest blocks via `n*&` argspec + LIFO BLOCK_STACK ([aa55556](https://github.com/elct9620/kobako/commit/aa55556aab23c159078d0ba0ea47ed878b26e89d))
* **rpc:** build block proxy for guest-supplied yield blocks ([b6d6cf7](https://github.com/elct9620/kobako/commit/b6d6cf7f5ca857f55aafea62631b243f688c61a6))
* **rpc:** catch/throw + frame invalidator close B-25 / B-28 / E-23 ([3b21f25](https://github.com/elct9620/kobako/commit/3b21f252fafdd2070f3953460509e24a0e643d88))
* **transport:** introduce empty Kobako::Transport namespace ([85cda26](https://github.com/elct9620/kobako/commit/85cda268000490f521424339bec1664d0b33478b))
* **wire:** add `block_given` field to Request envelope ([30e004f](https://github.com/elct9620/kobako/commit/30e004fa8f00739e68883889c5225c98cf9521fe))
* **wire:** add YieldResponse envelope codec on both sides ([4592567](https://github.com/elct9620/kobako/commit/459256784af616d70738ffd0f56c3b15244b3e7c))


### Bug Fixes

* **bench:** restore renamed class references so rake bench runs ([76140cc](https://github.com/elct9620/kobako/commit/76140cc99922973fc305aab6ba727a832ddbe7ba))
* **ext:** GC-root the dispatch Proc via a pinning mark on Kobako::Runtime ([f31bd07](https://github.com/elct9620/kobako/commit/f31bd071201b5fed7376bd13b876f103d6c6a5d6))
* **ext:** raise SandboxError, not TrapError, when #run envelope alloc fails ([a1981fe](https://github.com/elct9620/kobako/commit/a1981fea7438090a76758147e7e84543e9d96968))
* **transport:** fill E-xx placeholder and drop BLOCK_RESEARCH citations ([816ff80](https://github.com/elct9620/kobako/commit/816ff804535196036bec01fcd980e25036211b80))
* **wasm:** reject unrepresentable guest return values instead of stringifying ([c3fd069](https://github.com/elct9620/kobako/commit/c3fd0698cb168b55502fb86065406caf9a7744e1))
