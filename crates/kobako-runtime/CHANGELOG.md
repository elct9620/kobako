# Changelog

## [0.19.0](https://github.com/elct9620/kobako/compare/kobako-runtime-v0.18.0...kobako-runtime-v0.19.0) (2026-10-24)


### ⚠ BREAKING CHANGES

* **runtime:** a struct literal no longer builds OpenOptions, Metadata, DirEntry, Snapshot, Capture, or Usage outside kobako-runtime; use their `new` or `Default` and assign each field.
* the Guest ABI is version 5, so a Guest Binary built against version 4 is refused at Sandbox construction until rebuilt. The Yielder trait of kobako-runtime gains frame and takes the frame in yield_to_block; the Guest trait of kobako-core and the MrbGuest trait of kobako-mruby take the frame in yield_to_block; Downcall::Yield carries frame and args.
* **runtime:** the Runtime trait's invoke takes the invocation's Grants as a fourth argument, so every engine implementing it must accept them.

### Features

* check what a rename moves as it moves, refusing a directory beneath a Policy ([23794bf](https://github.com/elct9620/kobako/commit/23794bfcb54e148126e29873fa9b52d9e3de8d8c))
* **runtime:** bound the files and directories a Memory or Temporary holds ([8d0be0a](https://github.com/elct9620/kobako/commit/8d0be0ab3dc8c11bcb96865b0bf1a0bbb8dd3b16))
* **runtime:** bound what a Memory or Temporary holds, 64 MiB by default ([2557d1b](https://github.com/elct9620/kobako/commit/2557d1ba7530aacfddf908aeddac561c4fbc5c8a))
* **runtime:** draw every device a host does not name from one source ([210d55b](https://github.com/elct9620/kobako/commit/210d55b1b4e0699dc5ef56b4b6930be01658a3c6))
* **runtime:** hand each invocation its Grants and let an engine declare the kinds it supports ([cba90ee](https://github.com/elct9620/kobako/commit/cba90eed09c8afcb23080cef1c9cbb3650648472)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** keep a directory descriptor's identity its own while open ([6267642](https://github.com/elct9620/kobako/commit/6267642ea831917be76a0101e8d206ccfa8834c1))
* **runtime:** let a dispatch handler abandon its invocation ([179da9e](https://github.com/elct9620/kobako/commit/179da9ee1f31fed9e6d0c149faee0ad55f8db487))
* **runtime:** let a Filesystem's file refuse its close ([af16007](https://github.com/elct9620/kobako/commit/af1600797b098796dd6f329672c7d04e2e1cd2fa)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** let a Mount decide its own access as the one kind of Grant ([5c9189e](https://github.com/elct9620/kobako/commit/5c9189eea8ac61cf165dca1b38e459c4f14acdd0)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** let a Mount's Policy judge the spelling the host stores ([5281057](https://github.com/elct9620/kobako/commit/52810573f22d2a3edfae9b34b7767a5f440fba15))
* **runtime:** let the records a Filesystem or an engine builds gain fields ([c0ef88d](https://github.com/elct9620/kobako/commit/c0ef88d0b6df16d651e4c49226e41667307d772f))
* **runtime:** name a file grown past what a Filesystem keeps ([7bac3e6](https://github.com/elct9620/kobako/commit/7bac3e65534097b85d93f5967b6c73a979869a78))
* **runtime:** name each code a Filesystem may fail with ([1d980fa](https://github.com/elct9620/kobako/commit/1d980fa4328bad42e8a8665adfec9fc844690e97))
* **runtime:** name the POSIX error each FilesystemError stands for ([01c301b](https://github.com/elct9620/kobako/commit/01c301b4af9f489594eb7a98ab48c508e5b61ca4))
* **runtime:** refuse a Memory seed path through `..` ([b31434d](https://github.com/elct9620/kobako/commit/b31434d386ebde838c15755619c67da95311ecf6))
* **runtime:** refuse a Mount at a path that is no absolute guest path ([a525758](https://github.com/elct9620/kobako/commit/a5257589f9d831b9a3b6d1952df6e5c8469630e1))
* **runtime:** report a link a lookup does not follow as a link ([8c1e0b3](https://github.com/elct9620/kobako/commit/8c1e0b3e63fbc91b10f11cd91817ce94de24c7e6))
* **runtime:** report the identity of what a path holds ([392bc0e](https://github.com/elct9620/kobako/commit/392bc0e9f2fb935934a0031bf7d64dec0b27df6a))
* **runtime:** say whether a path beneath a Mount may pass a link ([7706a25](https://github.com/elct9620/kobako/commit/7706a2537bf8e5975b2b227bef0c05cb48c09f4b))
* **runtime:** seed a Memory from a list that may not hold, answering why ([8ac2dfd](https://github.com/elct9620/kobako/commit/8ac2dfdf73b84fe2e53405a653d861ddca9ea98c))
* **runtime:** ship the Memory, Temporary, Directory, and Overlay filesystems ([c60c453](https://github.com/elct9620/kobako/commit/c60c4535e966ffd501e03d7be7fef0a44d860b0b)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** stack an Overlay from Layers, each read-only or writable ([6c88188](https://github.com/elct9620/kobako/commit/6c88188cd7992c218f8c5a3be30981e685804078)), closes [#78](https://github.com/elct9620/kobako/issues/78)


### Bug Fixes

* hold an Overlay's lock across each call, waiting without the GVL ([b8e7342](https://github.com/elct9620/kobako/commit/b8e734273fdc577a40be97c43c46d9548d66c625))
* run a stored block as the block of the Call that carried it ([bf33dc1](https://github.com/elct9620/kobako/commit/bf33dc1fd40c83498c6c12b0be52b3a835a144cf))
* **runtime:** answer a host failure as the error naming it ([effd43a](https://github.com/elct9620/kobako/commit/effd43a85a3ba69aaa068d2c384c3b5cb466161c))
* **runtime:** append to a Directory file in one host write ([4a5450a](https://github.com/elct9620/kobako/commit/4a5450a67e202e45299175c4fe7528cf50489f0e))
* **runtime:** copy a file up whole when its Layer writes in parts ([89d87f7](https://github.com/elct9620/kobako/commit/89d87f7225611786467968ec34eba5a8223b5fc5))
* **runtime:** fail an Overlay call a Layer cannot answer instead of guessing ([5ba0db3](https://github.com/elct9620/kobako/commit/5ba0db33c56ed837401bcb62345f81b4b50c317a))
* **runtime:** judge a link an Overlay's Layer holds as the link itself ([959c1f3](https://github.com/elct9620/kobako/commit/959c1f3dab6f6948baabbd914bbb86dfff090f7b))
* **runtime:** leave an Overlay unchanged when a removal or rename is refused ([a7bee4b](https://github.com/elct9620/kobako/commit/a7bee4b2a465a0ef0ad2e376c281dca82a0df0e1))
* **runtime:** let a file or link in a Layer hide what lies below it ([fd579f6](https://github.com/elct9620/kobako/commit/fd579f679cfa8a83ec7d139066bc1a4570af398b))
* **runtime:** move a directory made again as the writable Layer's own ([ac3e864](https://github.com/elct9620/kobako/commit/ac3e864bbb0ce5d61def45e0403239be405481d3))
* **runtime:** move a directory onto one that holds only markers ([934f0c3](https://github.com/elct9620/kobako/commit/934f0c39e0ad1189bdb1cb0461671245dea32f93))
* **runtime:** open a Directory file the runtime neither reads nor writes ([6b30858](https://github.com/elct9620/kobako/commit/6b308583383d6803894b2c7af1a9ef05d72b0e7d)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** open a file already there without writing to create it ([7f76a1a](https://github.com/elct9620/kobako/commit/7f76a1af575739ae16c5daeb1df8268af44cea63))
* **runtime:** panic on files a Memory cannot be seeded with ([cf5c835](https://github.com/elct9620/kobako/commit/cf5c835d7df8bf82972487280c6a6c79be955a68))
* **runtime:** put an Overlay's markers back when the change fails ([78ddeed](https://github.com/elct9620/kobako/commit/78ddeeddda8b9f797a80ec3796b28e04cf99ffc9))
* **runtime:** refuse a backslash in a Directory path on Windows ([88b5719](https://github.com/elct9620/kobako/commit/88b57195325b168395248a82b94a5ebf7e5f4142))
* **runtime:** refuse a file moved beneath itself as no directory ([bfabb52](https://github.com/elct9620/kobako/commit/bfabb52b27080ee9e5000bd5524ea201a7e33d80))
* **runtime:** refuse a host name that is not UTF-8 as an illegal sequence ([c4ff030](https://github.com/elct9620/kobako/commit/c4ff030ffa8e29fa1b10df4aaf6af65162c76353))
* **runtime:** refuse a Memory file grown past what can be allocated ([302e2d4](https://github.com/elct9620/kobako/commit/302e2d4fcd5b31fee4e953e8993bb47d9a2a673a))
* **runtime:** refuse an Overlay change landing beneath a read-only Layer ([e7b2f19](https://github.com/elct9620/kobako/commit/e7b2f19156df9773f669cbe8a7aaefc17591cca3))
* **runtime:** remove no directory as a file on any host ([c9c0820](https://github.com/elct9620/kobako/commit/c9c0820fd39c0744b9d9c367b977f6587d1f9589))
* **runtime:** rename and copy up through an Overlay as overlayfs does ([ada5a86](https://github.com/elct9620/kobako/commit/ada5a86a8647d89666e9de808eb13ba638882e51))
* **runtime:** report a merged directory's identity alike through an Overlay's lstat ([ded3b4a](https://github.com/elct9620/kobako/commit/ded3b4aba78efa678318c38b139b5c9c76b5d3fd))
* **runtime:** require cap-std 4.0.3 for the trailing-slash lookup fix ([549cfe0](https://github.com/elct9620/kobako/commit/549cfe07fc8b9affdc8a1765a9e4bc5963ef546e))
* **runtime:** reserve an Overlay's marker name in any case ([ef39923](https://github.com/elct9620/kobako/commit/ef399230eabe01ad6eed632b2753880ae1fc322c))
* **runtime:** reserve exactly what a Memory file grows by ([96f8141](https://github.com/elct9620/kobako/commit/96f8141442cac2170a8bf8a76055d4f4659b1100))
* **wasmtime:** hold the whole invocation to its deadline ([231e325](https://github.com/elct9620/kobako/commit/231e32592699008adf3105050bee8b5528ea27a8))
* **wasmtime:** place an append at the end as the file stands when it lands ([d7291c4](https://github.com/elct9620/kobako/commit/d7291c4f32fb1f5f2494c69c76796476bbcf4d43))
* **wasmtime:** refuse a link an open asks not to follow ([123a01b](https://github.com/elct9620/kobako/commit/123a01b01f59cc69c0e3a23f9aeb01aa89132c07))


### Performance Improvements

* **runtime:** stop an Overlay's marker lookup at a directory a Layer lacks ([494fa09](https://github.com/elct9620/kobako/commit/494fa090e90806c50e4e8baaa4f1001efc388b45))

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
