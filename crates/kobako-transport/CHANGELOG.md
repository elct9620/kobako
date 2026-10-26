# Changelog

## [0.19.0](https://github.com/elct9620/kobako/compare/kobako-transport-v0.18.0...kobako-transport-v0.19.0) (2026-10-26)


### ⚠ BREAKING CHANGES

* the Guest ABI is version 5, so a Guest Binary built against version 4 is refused at Sandbox construction until rebuilt. In kobako-transport, Reply gains a Break arm and YieldReply::Break carries no payload. The Yielder trait of kobako-runtime gains frame and takes the frame in yield_to_block; the Guest trait of kobako-core and the MrbGuest trait of kobako-mruby take the frame in yield_to_block; Downcall::Yield carries frame and args.

### Features

* let a block's break or return unwind a Service as a plain method ([4e9eca0](https://github.com/elct9620/kobako/commit/4e9eca05f1647c8b6409fc00d08317b431c0c648))


### Bug Fixes

* run a stored block as the block of the Call that carried it ([bf33dc1](https://github.com/elct9620/kobako/commit/bf33dc1fd40c83498c6c12b0be52b3a835a144cf))

## [0.18.0](https://github.com/elct9620/kobako/compare/kobako-transport-v0.17.0...kobako-transport-v0.18.0) (2026-10-13)


### ⚠ BREAKING CHANGES

* **transport:** the stdin layout a Guest Binary reads changed within ABI 4. kobako_transport::envelope::Frames is replaced by Preamble, which is #[non_exhaustive] and built from Default; Bindings and Snippets no longer have their own decode and encode. Guest::eval and Guest::run, MrbGuest::eval and MrbGuest::run, and Runtime::invoke take a Preamble in place of Frames. kobako_core re-exports Preamble and Flag in place of Frames.
* **mruby:** CodecError::Unsupported is renamed to CodecError::Unserved; a codec of one's own that refuses a position returns the new name.
* **transport:** the ABI version is 4, so a Guest Binary built for ABI 3 is refused at Sandbox construction. kobako_core::export_guest! emits __kobako_downcall in place of __kobako_eval, __kobako_run, __kobako_yield_to_block and __kobako_take_outcome, and the host import is __kobako_upcall. kobako_core::abi::{take_outcome, write_outcome, write_reply, decode_run} are replaced by serve and answer, and the frames module is no longer public. Reply, YieldReply and Outcome gain an Unsupported arm and take their tags from the shared registry, and kobako_core::DispatchError and kobako::YieldError gain an Unsupported arm. A yield to a guest without block support raises Kobako::UnsupportedError at the Service's yield instead of failing the invocation as a trap.

### Features

* **transport:** carry a compile request and its answer ([f30958e](https://github.com/elct9620/kobako/commit/f30958e7d61c95667c32cfee7d9b2d2f74550a8e)), closes [#75](https://github.com/elct9620/kobako/issues/75)
* **transport:** carry each operation as a tag on one channel per direction ([c7d574b](https://github.com/elct9620/kobako/commit/c7d574b59a42e58632f2bdc46cc9a8e4c7ac6121)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** keep an unknown request's body, and refuse tag 0x00 ([edb64df](https://github.com/elct9620/kobako/commit/edb64df8f87694d17c5b6fd71ac63329023306ec)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** name the Frames a Sandbox is set up with ([52b25aa](https://github.com/elct9620/kobako/commit/52b25aa48d321ce8ea7dc9532cfade062118c64f)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** set an invocation up from a Preamble of tagged records ([84d4498](https://github.com/elct9620/kobako/commit/84d4498464f94d7312f26931ddd4ec43894a6654))


### Bug Fixes

* **transport:** read only an unassigned tag as an unknown request ([fcb81b5](https://github.com/elct9620/kobako/commit/fcb81b5c38d001b729a10287443c5012281b76b6)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** size a counted list by what its bytes can hold ([cfcc9fb](https://github.com/elct9620/kobako/commit/cfcc9fb6e79ec0bd1e6bf3a15659220aab0f6914))


### Code Refactoring

* **mruby:** name a codec's refused position Unserved ([91979f9](https://github.com/elct9620/kobako/commit/91979f9546a3fa2d2dec8c136ed6affc521ba6fb)), closes [#73](https://github.com/elct9620/kobako/issues/73)

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-transport-v0.16.0...kobako-transport-v0.17.0) (2026-09-20)


### Miscellaneous Chores

* **kobako-transport:** Synchronize kobako crates versions

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-transport-v0.15.0...kobako-transport-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-transport:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-transport-v0.14.0...kobako-transport-v0.15.0) (2026-09-13)


### Features

* **spec:** declare the bytes both sides must already agree on ([3875755](https://github.com/elct9620/kobako/commit/3875755999550511837808dcd24a25086bedc926))

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-transport-v0.13.1...kobako-transport-v0.14.0) (2026-08-06)


### Features

* **transport:** let a guest block's exception continue as itself ([c8b1de7](https://github.com/elct9620/kobako/commit/c8b1de7154336fab8fa20c0d7a5b3baaad11d551))
* **transport:** separate a failed exchange from a failed Service ([fb92515](https://github.com/elct9620/kobako/commit/fb92515fc76ffebbb8491b525d53b1725cf97dd0))


### Bug Fixes

* **wire:** let a Fault reader survive what it predates ([eeabfb7](https://github.com/elct9620/kobako/commit/eeabfb7aa4de3d08852ca1b41517b924d72a7312))

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-transport-v0.13.0...kobako-transport-v0.13.1) (2026-07-30)


### Miscellaneous Chores

* **kobako-transport:** Synchronize kobako crates versions

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-transport-v0.12.0...kobako-transport-v0.13.0) (2026-07-29)


### ⚠ BREAKING CHANGES

* **runtime:** mark the sets that grow, and say why the closed ones do not
* **transport:** let every envelope spell its success arm the same way
* **transport:** name the core envelope's types after what they carry
* **wire:** carry a Reply's fault arm on the envelope
* **transport:** split the invocation module along the line its doc draws

### Features

* **runtime:** mark the sets that grow, and say why the closed ones do not ([1be2449](https://github.com/elct9620/kobako/commit/1be24492961e2c2a3a317f784cc2f20ae584dcbf))
* **transport:** add kobako-transport, one implementation of the envelope ([23f2401](https://github.com/elct9620/kobako/commit/23f240111a4c59e8f521bec911f9cf1cd7287744))
* **transport:** give the ABI's fixed values one definition each ([94754ec](https://github.com/elct9620/kobako/commit/94754ecb538286077b6b58d90504b458ac0e4a2c))
* **wire:** carry a Reply's fault arm on the envelope ([0bce850](https://github.com/elct9620/kobako/commit/0bce850b3416696e92ae6ee12c353c4c21c8e583))


### Bug Fixes

* **wire:** keep the ABI at 3, which no release has shipped ([2b301b7](https://github.com/elct9620/kobako/commit/2b301b777384c0edf79426805cfb1cc08688aa95))


### Code Refactoring

* **transport:** let every envelope spell its success arm the same way ([df595e0](https://github.com/elct9620/kobako/commit/df595e08097af9e3e0a0c5ed52a4a6f854eb21dc))
* **transport:** name the core envelope's types after what they carry ([b44de65](https://github.com/elct9620/kobako/commit/b44de6502953826c567d4ef8a594561479f50c1d))
* **transport:** split the invocation module along the line its doc draws ([4d3a2d3](https://github.com/elct9620/kobako/commit/4d3a2d3060167d71a807e203688186740fd1485b))
