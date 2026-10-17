# Changelog

## [0.19.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.18.0...kobako-runtime-v0.19.0) (2026-10-17)


### ⚠ BREAKING CHANGES

* **runtime:** the Runtime trait's invoke takes the invocation's Grants as a fourth argument, so every engine implementing it must accept them.

### Features

* **runtime:** hand each invocation its Grants and let an engine declare the kinds it supports ([cba90ee](https://github.com/elct9620/kobako/commit/cba90eed09c8afcb23080cef1c9cbb3650648472)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** let a Mount decide its own access as the one kind of Grant ([5c9189e](https://github.com/elct9620/kobako/commit/5c9189eea8ac61cf165dca1b38e459c4f14acdd0)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** ship the Memory, Temporary, Directory, and Overlay filesystems ([c60c453](https://github.com/elct9620/kobako/commit/c60c4535e966ffd501e03d7be7fef0a44d860b0b)), closes [#78](https://github.com/elct9620/kobako/issues/78)


### Bug Fixes

* **runtime:** open a Directory file the runtime neither reads nor writes ([6b30858](https://github.com/elct9620/kobako/commit/6b308583383d6803894b2c7af1a9ef05d72b0e7d)), closes [#78](https://github.com/elct9620/kobako/issues/78)

## [0.18.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.17.0...kobako-runtime-v0.18.0) (2026-10-13)


### ⚠ BREAKING CHANGES

* **transport:** the stdin layout a Guest Binary reads changed within ABI 4. kobako_transport::envelope::Frames is replaced by Preamble, which is #[non_exhaustive] and built from Default; Bindings and Snippets no longer have their own decode and encode. Guest::eval and Guest::run, MrbGuest::eval and MrbGuest::run, and Runtime::invoke take a Preamble in place of Frames. kobako_core re-exports Preamble and Flag in place of Frames.
* **transport:** the ABI version is 4, so a Guest Binary built for ABI 3 is refused at Sandbox construction. kobako_core::export_guest! emits __kobako_downcall in place of __kobako_eval, __kobako_run, __kobako_yield_to_block and __kobako_take_outcome, and the host import is __kobako_upcall. kobako_core::abi::{take_outcome, write_outcome, write_reply, decode_run} are replaced by serve and answer, and the frames module is no longer public. Reply, YieldReply and Outcome gain an Unsupported arm and take their tags from the shared registry, and kobako_core::DispatchError and kobako::YieldError gain an Unsupported arm. A yield to a guest without block support raises Kobako::UnsupportedError at the Service's yield instead of failing the invocation as a trap.
* **runtime:** Runtime::invoke takes &kobako_transport::envelope::Frames in place of kobako_runtime::Frames, whose byte fields are gone. Guest::eval and Guest::run (and their MrbGuest flows) receive the decoded Frames, eval also its source; kobako_core::frames::read_frame is replaced by read_eval_frames and read_frames.
* **runtime:** kobako_runtime::snapshot::Completion::Outcome holds a decoded kobako_transport::envelope::Outcome instead of Vec<u8>; an engine decodes the result itself and reports an unframed one as
* **runtime:** kobako_runtime::runtime::Entry::Run holds { entrypoint: &str, payload: Vec<u8> } instead of { envelope: &[u8] }; a frontend passes the fields, and an engine encodes the Run envelope itself.
* **runtime:** kobako_runtime::yielder::Yielder::yield_to_block returns Result<YieldReply, Trap> instead of the encoded bytes; an engine implementing it decodes the reply itself.

### Features

* **runtime:** compile a Preamble's snippets on an instance of their own ([303432d](https://github.com/elct9620/kobako/commit/303432dd90ed62dc378391515e68e784e1b473a4)), closes [#75](https://github.com/elct9620/kobako/issues/75)
* **transport:** carry each operation as a tag on one channel per direction ([c7d574b](https://github.com/elct9620/kobako/commit/c7d574b59a42e58632f2bdc46cc9a8e4c7ac6121)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** set an invocation up from a Preamble of tagged records ([84d4498](https://github.com/elct9620/kobako/commit/84d4498464f94d7312f26931ddd4ec43894a6654))


### Bug Fixes

* keep a compile's bytecode only when the Preamble still fits it ([765db08](https://github.com/elct9620/kobako/commit/765db08ad31c8f5163794c7e144355f3237af93b))


### Code Refactoring

* **runtime:** hand a frontend the decoded Outcome ([0da6aac](https://github.com/elct9620/kobako/commit/0da6aac0f7943d694a5a4ab9439b2d6c527ae1b0)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** hand a frontend the decoded Yield Reply ([e80ab54](https://github.com/elct9620/kobako/commit/e80ab54e36c2a80b0f746c675f85bd1238fd4331)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** let the ABI endpoints frame and read the Frames ([dea069a](https://github.com/elct9620/kobako/commit/dea069ac49a59c12a6f7102ed3cf2925f316cbff)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** let the engine frame a run's entrypoint and payload ([62f9c94](https://github.com/elct9620/kobako/commit/62f9c9407ff3ce690723625bea075fa49d2dda47)), closes [#73](https://github.com/elct9620/kobako/issues/73)

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.16.0...kobako-runtime-v0.17.0) (2026-09-20)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.15.0...kobako-runtime-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.14.0...kobako-runtime-v0.15.0) (2026-09-13)


### Features

* **spec:** declare what the guest answers when a value will not cross ([886a16f](https://github.com/elct9620/kobako/commit/886a16f173efe44766dd9884ac8bfed61113728a))

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.13.1...kobako-runtime-v0.14.0) (2026-08-06)


### Miscellaneous Chores

* **kobako-runtime:** Synchronize kobako crates versions

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
