# Changelog

## [0.19.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.18.0...kobako-core-v0.19.0) (2026-10-24)


### ⚠ BREAKING CHANGES

* in kobako-transport, YieldReply::Break carries no payload and Reply gains a Break arm, so a Guest Binary built against the earlier version 5 wire must be rebuilt.
* the Guest ABI is version 5, so a Guest Binary built against version 4 is refused at Sandbox construction until rebuilt. The Yielder trait of kobako-runtime gains frame and takes the frame in yield_to_block; the Guest trait of kobako-core and the MrbGuest trait of kobako-mruby take the frame in yield_to_block; Downcall::Yield carries frame and args.

### Features

* let a block's break or return unwind a Service as a plain method ([4e9eca0](https://github.com/elct9620/kobako/commit/4e9eca05f1647c8b6409fc00d08317b431c0c648))


### Bug Fixes

* run a stored block as the block of the Call that carried it ([bf33dc1](https://github.com/elct9620/kobako/commit/bf33dc1fd40c83498c6c12b0be52b3a835a144cf))

## [0.18.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.17.0...kobako-core-v0.18.0) (2026-10-13)


### ⚠ BREAKING CHANGES

* **transport:** the stdin layout a Guest Binary reads changed within ABI 4. kobako_transport::envelope::Frames is replaced by Preamble, which is #[non_exhaustive] and built from Default; Bindings and Snippets no longer have their own decode and encode. Guest::eval and Guest::run, MrbGuest::eval and MrbGuest::run, and Runtime::invoke take a Preamble in place of Frames. kobako_core re-exports Preamble and Flag in place of Frames.
* **mruby:** CodecError::Unsupported is renamed to CodecError::Unserved; a codec of one's own that refuses a position returns the new name.
* **transport:** the ABI version is 4, so a Guest Binary built for ABI 3 is refused at Sandbox construction. kobako_core::export_guest! emits __kobako_downcall in place of __kobako_eval, __kobako_run, __kobako_yield_to_block and __kobako_take_outcome, and the host import is __kobako_upcall. kobako_core::abi::{take_outcome, write_outcome, write_reply, decode_run} are replaced by serve and answer, and the frames module is no longer public. Reply, YieldReply and Outcome gain an Unsupported arm and take their tags from the shared registry, and kobako_core::DispatchError and kobako::YieldError gain an Unsupported arm. A yield to a guest without block support raises Kobako::UnsupportedError at the Service's yield instead of failing the invocation as a trap.
* **runtime:** Runtime::invoke takes &kobako_transport::envelope::Frames in place of kobako_runtime::Frames, whose byte fields are gone. Guest::eval and Guest::run (and their MrbGuest flows) receive the decoded Frames, eval also its source; kobako_core::frames::read_frame is replaced by read_eval_frames and read_frames.
* **guest:** kobako_core::Guest::eval and ::run return Outcome, ::run takes a Run instead of the envelope bytes, and ::yield_to_block returns a YieldReply instead of a packed pointer; kobako_mruby::MrbGuest mirrors these signatures. kobako_core::abi::write_outcome takes an Outcome, write_panic is removed, and decode_run and write_reply are new.

### Features

* **guest:** compile a Preamble's source snippets without running them ([1c649b8](https://github.com/elct9620/kobako/commit/1c649b809c6d72c52f437cd19a89c1972201a5f0)), closes [#75](https://github.com/elct9620/kobako/issues/75)
* **guest:** re-export the messages a Guest takes and answers with ([89aefee](https://github.com/elct9620/kobako/commit/89aefee5ac18115252fd7a0680c4190e16a231cf)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** carry a compile request and its answer ([f30958e](https://github.com/elct9620/kobako/commit/f30958e7d61c95667c32cfee7d9b2d2f74550a8e)), closes [#75](https://github.com/elct9620/kobako/issues/75)
* **transport:** carry each operation as a tag on one channel per direction ([c7d574b](https://github.com/elct9620/kobako/commit/c7d574b59a42e58632f2bdc46cc9a8e4c7ac6121)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** keep an unknown request's body, and refuse tag 0x00 ([edb64df](https://github.com/elct9620/kobako/commit/edb64df8f87694d17c5b6fd71ac63329023306ec)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** set an invocation up from a Preamble of tagged records ([84d4498](https://github.com/elct9620/kobako/commit/84d4498464f94d7312f26931ddd4ec43894a6654))


### Bug Fixes

* word an unwritable Service answer as the gem does and an empty Reply as a Reply ([200beb5](https://github.com/elct9620/kobako/commit/200beb5f472118061ee3897f489190ca090b92a8))


### Code Refactoring

* **guest:** let export_guest! frame what a guest answers ([1ec566f](https://github.com/elct9620/kobako/commit/1ec566f9f5c6b29c07f9293df344ce7e13fb7182)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **mruby:** name a codec's refused position Unserved ([91979f9](https://github.com/elct9620/kobako/commit/91979f9546a3fa2d2dec8c136ed6affc521ba6fb)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** let the ABI endpoints frame and read the Frames ([dea069a](https://github.com/elct9620/kobako/commit/dea069ac49a59c12a6f7102ed3cf2925f316cbff)), closes [#73](https://github.com/elct9620/kobako/issues/73)

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.16.0...kobako-core-v0.17.0) (2026-09-20)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako crates versions

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.15.0...kobako-core-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.14.0...kobako-core-v0.15.0) (2026-09-13)


### Features

* **spec:** declare the bytes both sides must already agree on ([3875755](https://github.com/elct9620/kobako/commit/3875755999550511837808dcd24a25086bedc926))
* **spec:** declare what the guest answers when a value will not cross ([886a16f](https://github.com/elct9620/kobako/commit/886a16f173efe44766dd9884ac8bfed61113728a))

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-core-v0.13.1...kobako-core-v0.14.0) (2026-08-06)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako crates versions

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-core-v0.13.0...kobako-core-v0.13.1) (2026-07-30)


### Miscellaneous Chores

* **kobako-core:** Synchronize kobako crates versions

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
