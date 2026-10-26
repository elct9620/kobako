# Changelog

## [0.19.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.18.0...kobako-mruby-v0.19.0) (2026-10-26)


### ⚠ BREAKING CHANGES

* in kobako-transport, YieldReply::Break carries no payload and Reply gains a Break arm, so a Guest Binary built against the earlier version 5 wire must be rebuilt.
* kobako-codec renames `Decoder::at_end` to `is_at_end`, `Decoder::read_only_value` to `read_sole_value`, and `Decoder::with_value_limit` to `with_value_budget`; the kobako crate renames `handles::Detached` to `handles::DetachedHandles`.
* the Guest ABI is version 5, so a Guest Binary built against version 4 is refused at Sandbox construction until rebuilt. The Yielder trait of kobako-runtime gains frame and takes the frame in yield_to_block; the Guest trait of kobako-core and the MrbGuest trait of kobako-mruby take the frame in yield_to_block; Downcall::Yield carries frame and args.
* **guest:** kobako-mruby drops the narrow_int method on Kobako, the IntegerOutOfRange type and the Interpreter variant of CodecError; a codec builds a guest Integer from a wire integer through beni's IntoValue.
* **guest:** the kobako guest crates name beni 0.25's types, so a shell or codec built on them moves to beni 0.25 with them.
* **guest:** the default memory_limit is now 4 MiB. JSON.parse in the guest answers an Integer for every integer within 64 bits, where it used to answer a Float past 32 bits. An integer past 64 bits now raises
* **guest:** the kobako guest crates name beni 0.24's types, so a shell or codec built on them moves to beni 0.24 with them.

### Features

* **guest:** carry every integer msgpack encodes in both directions ([ec349f9](https://github.com/elct9620/kobako/commit/ec349f98fcb38443d9f9dfc45701e2ce87ff7710))
* **guest:** carry mruby-bigint so an Integer has no width of its own ([a74266d](https://github.com/elct9620/kobako/commit/a74266d3e7b1295b75770b62894b232195f9b3bb))
* **guest:** widen the guest Integer to 64 bits ([7236a26](https://github.com/elct9620/kobako/commit/7236a26285dfdef175048f659c1094dd3bbb5b4d))
* let a block's break or return unwind a Service as a plain method ([4e9eca0](https://github.com/elct9620/kobako/commit/4e9eca05f1647c8b6409fc00d08317b431c0c648))


### Bug Fixes

* **guest:** name the segment a bind path meets when the Guest Binary already defines it ([0dc1766](https://github.com/elct9620/kobako/commit/0dc1766ff60a1ef03a5b0c337dfd02041fb3fb8f))
* run a stored block as the block of the Call that carried it ([bf33dc1](https://github.com/elct9620/kobako/commit/bf33dc1fd40c83498c6c12b0be52b3a835a144cf))


### Code Refactoring

* name the value budget and decoder queries by the conventions ([918f748](https://github.com/elct9620/kobako/commit/918f7488dea13d27def92dca9e31128d90856f1c))


### Build System

* **guest:** rebuild the guest crates on beni 0.24 ([4599337](https://github.com/elct9620/kobako/commit/4599337261d1d28ff8c36435062d34cbccf09abb))
* **guest:** rebuild the guest crates on beni 0.25 ([a06dfd2](https://github.com/elct9620/kobako/commit/a06dfd25958ded9c82072149d73e56372343ba31))

## [0.18.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.17.0...kobako-mruby-v0.18.0) (2026-10-13)


### ⚠ BREAKING CHANGES

* **guest:** the kobako guest crates name beni 0.23's types, so a shell or codec built on them moves to beni 0.23 with them.
* **guest:** preloaded bytecode whose top level raises ScriptError itself now fails as a SandboxError carrying ScriptError, not as
* **transport:** the stdin layout a Guest Binary reads changed within ABI 4. kobako_transport::envelope::Frames is replaced by Preamble, which is #[non_exhaustive] and built from Default; Bindings and Snippets no longer have their own decode and encode. Guest::eval and Guest::run, MrbGuest::eval and MrbGuest::run, and Runtime::invoke take a Preamble in place of Frames. kobako_core re-exports Preamble and Flag in place of Frames.
* **mruby:** CodecError::Unsupported is renamed to CodecError::Unserved; a codec of one's own that refuses a position returns the new name.
* **transport:** the ABI version is 4, so a Guest Binary built for ABI 3 is refused at Sandbox construction. kobako_core::export_guest! emits __kobako_downcall in place of __kobako_eval, __kobako_run, __kobako_yield_to_block and __kobako_take_outcome, and the host import is __kobako_upcall. kobako_core::abi::{take_outcome, write_outcome, write_reply, decode_run} are replaced by serve and answer, and the frames module is no longer public. Reply, YieldReply and Outcome gain an Unsupported arm and take their tags from the shared registry, and kobako_core::DispatchError and kobako::YieldError gain an Unsupported arm. A yield to a guest without block support raises Kobako::UnsupportedError at the Service's yield instead of failing the invocation as a trap.
* **runtime:** Runtime::invoke takes &kobako_transport::envelope::Frames in place of kobako_runtime::Frames, whose byte fields are gone. Guest::eval and Guest::run (and their MrbGuest flows) receive the decoded Frames, eval also its source; kobako_core::frames::read_frame is replaced by read_eval_frames and read_frames.
* **guest:** kobako_core::Guest::eval and ::run return Outcome, ::run takes a Run instead of the envelope bytes, and ::yield_to_block returns a YieldReply instead of a packed pointer; kobako_mruby::MrbGuest mirrors these signatures. kobako_core::abi::write_outcome takes an Outcome, write_panic is removed, and decode_run and write_reply are new.
* **mruby:** Kobako::set_handle_id is removed. A custom flow that needs a Handle calls Kobako::mint_handle, which caps the id and returns a frozen Kobako::Handle.
* **guest:** the kobako guest crates name beni 0.22's types, so a shell or codec built on them moves to beni 0.22 with them.
* **guest:** the kobako-mruby harness names beni 0.21's types — the keyword Hash in `Arguments` and `PayloadCodec::encode_call_arguments` is `beni::RHash` — so a shell or codec built on these crates moves to beni 0.21 with them.
* **guest:** a pattern's allocator is undefined from the moment the capability installs rather than from its first wrap, so allocating one is refused for the missing allocator throughout an invocation, where before the first statement could still reach an uninitialized carrier.
* **mruby:** `kobako_mruby::dispatch` takes `Option<Proc>` where it took a `Value` that could be nil. A gem passes the block its method read off its own frame; `None` says there is none.
* **mruby:** an object bearing a capability reference's class name without being one is refused as a value the wire cannot carry, where it previously crossed as a reference to whatever identifier it held.
* **mruby:** an object whose class carries a list's or a map's name without being one is refused rather than read as that container, both as the invocation's own answer and as a dispatch argument.
* **mruby:** `Kobako::Handle#initialize_copy` answers an ArgumentError for any count but one. The name is private, as it is in MRI, so reaching it at all takes a `send`; `dup` and `clone` pass their one argument and are unaffected.
* **guest:** `Kobako::raise_transport_error`, `raise_service_error` and `reraise` are gone. A flow of its own builds `Kobako::transport_error` or `service_error` and hands the result back as `Err`, which beni raises at the guest call site.

### Features

* **guest:** compile a Preamble's source snippets without running them ([1c649b8](https://github.com/elct9620/kobako/commit/1c649b809c6d72c52f437cd19a89c1972201a5f0)), closes [#75](https://github.com/elct9620/kobako/issues/75)
* **guest:** raise Kobako::UnsupportedError when a host does not serve a call ([db003af](https://github.com/elct9620/kobako/commit/db003af75cf2d11b5f644986aa77c0408f5f59ba)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **guest:** re-export the messages a Guest takes and answers with ([89aefee](https://github.com/elct9620/kobako/commit/89aefee5ac18115252fd7a0680c4190e16a231cf)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **guest:** rebuild the guest crates on beni 0.17 ([b397701](https://github.com/elct9620/kobako/commit/b397701231c7d8718503d24f73b175676d0d1be3))
* **guest:** rebuild the guest crates on beni 0.18 ([ae232eb](https://github.com/elct9620/kobako/commit/ae232ebbf708419ff0672172fbf8627363fb02da))
* **guest:** rebuild the guest crates on beni 0.21 ([8ec00ff](https://github.com/elct9620/kobako/commit/8ec00ff94ec21b6bbfe8b88e0a4e75f4489dd692))
* **mruby:** upgrade beni to 0.14 and locate a parse failure ([586c910](https://github.com/elct9620/kobako/commit/586c910873710169c0dfce1fe7edb21f9e91cdea))
* **spec:** account for every error scenario the old anchors stated ([179de50](https://github.com/elct9620/kobako/commit/179de5040c54bf3800d3d6ba02b56cf4eaed685b))
* **spec:** declare what the guest answers when a value will not cross ([886a16f](https://github.com/elct9620/kobako/commit/886a16f173efe44766dd9884ac8bfed61113728a))
* **transport:** carry each operation as a tag on one channel per direction ([c7d574b](https://github.com/elct9620/kobako/commit/c7d574b59a42e58632f2bdc46cc9a8e4c7ac6121)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** set an invocation up from a Preamble of tagged records ([84d4498](https://github.com/elct9620/kobako/commit/84d4498464f94d7312f26931ddd4ec43894a6654))


### Bug Fixes

* **guest:** mint a Handle without running the initializer the guest can redefine ([12ed1aa](https://github.com/elct9620/kobako/commit/12ed1aa07517b6f0ca6fa0869248206f3bb18b81))
* **guest:** tell an unreadable bytecode snippet from one that raised ([fc5156d](https://github.com/elct9620/kobako/commit/fc5156d783d5ab21164df533bc97ec4afcf0ca09))
* **mruby:** hold the Handle copy hook to the one argument it takes ([e46da94](https://github.com/elct9620/kobako/commit/e46da944243a51291ddb1f398fc254b44fa6fab0))
* **mruby:** prove a container's layout by its tag, not its class name ([5ee8c59](https://github.com/elct9620/kobako/commit/5ee8c59db90cee27f356028ff466860afe89e06e))
* **mruby:** read a reference's identity from the class the bridge registered ([0f24102](https://github.com/elct9620/kobako/commit/0f24102b75f771548cbc39256690e59d6df1f9e4))
* **spec:** claim the scenario each test actually witnesses ([ab92e8b](https://github.com/elct9620/kobako/commit/ab92e8b9f5948f16fb86e517b9501598d76e4b89))


### Performance Improvements

* **mruby:** walk a Hash's pairs once when encoding it ([1737058](https://github.com/elct9620/kobako/commit/1737058ae2bdeeeafe1f0b8e0edfa1b98b09e776))


### Code Refactoring

* **guest:** let export_guest! frame what a guest answers ([1ec566f](https://github.com/elct9620/kobako/commit/1ec566f9f5c6b29c07f9293df344ce7e13fb7182)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **mruby:** carry a dispatch's block as the block it is ([01d8831](https://github.com/elct9620/kobako/commit/01d8831dff173e80c10d965af8615acb4de2498b))
* **mruby:** fold Kobako::set_handle_id into mint_handle ([3a47d66](https://github.com/elct9620/kobako/commit/3a47d66eef486bdd7fc1dd546b2eed8d5d5226ea))
* **mruby:** name a codec's refused position Unserved ([91979f9](https://github.com/elct9620/kobako/commit/91979f9546a3fa2d2dec8c136ed6affc521ba6fb)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** let the ABI endpoints frame and read the Frames ([dea069a](https://github.com/elct9620/kobako/commit/dea069ac49a59c12a6f7102ed3cf2925f316cbff)), closes [#73](https://github.com/elct9620/kobako/issues/73)


### Build System

* **guest:** rebuild the guest crates on beni 0.22 ([fd6aeb9](https://github.com/elct9620/kobako/commit/fd6aeb92705fa53c52e2cd75b4918a58baa6637e))
* **guest:** rebuild the guest crates on beni 0.23 ([a6782da](https://github.com/elct9620/kobako/commit/a6782dad4c2c68a25573f82a8a16588c09856d23))

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.16.0...kobako-mruby-v0.17.0) (2026-09-20)


### ⚠ BREAKING CHANGES

* **guest:** a pattern's allocator is undefined from the moment the capability installs rather than from its first wrap, so allocating one is refused for the missing allocator throughout an invocation, where before the first statement could still reach an uninitialized carrier.
* **mruby:** `kobako_mruby::dispatch` takes `Option<Proc>` where it took a `Value` that could be nil. A gem passes the block its method read off its own frame; `None` says there is none.
* **mruby:** an object bearing a capability reference's class name without being one is refused as a value the wire cannot carry, where it previously crossed as a reference to whatever identifier it held.
* **mruby:** an object whose class carries a list's or a map's name without being one is refused rather than read as that container, both as the invocation's own answer and as a dispatch argument.
* **mruby:** `Kobako::Handle#initialize_copy` answers an ArgumentError for any count but one. The name is private, as it is in MRI, so reaching it at all takes a `send`; `dup` and `clone` pass their one argument and are unaffected.
* **guest:** `Kobako::raise_transport_error`, `raise_service_error` and `reraise` are gone. A flow of its own builds `Kobako::transport_error` or `service_error` and hands the result back as `Err`, which beni raises at the guest call site.

### Features

* **guest:** rebuild the guest crates on beni 0.17 ([b397701](https://github.com/elct9620/kobako/commit/b397701231c7d8718503d24f73b175676d0d1be3))
* **guest:** rebuild the guest crates on beni 0.18 ([ae232eb](https://github.com/elct9620/kobako/commit/ae232ebbf708419ff0672172fbf8627363fb02da))


### Bug Fixes

* **mruby:** hold the Handle copy hook to the one argument it takes ([e46da94](https://github.com/elct9620/kobako/commit/e46da944243a51291ddb1f398fc254b44fa6fab0))
* **mruby:** prove a container's layout by its tag, not its class name ([5ee8c59](https://github.com/elct9620/kobako/commit/5ee8c59db90cee27f356028ff466860afe89e06e))
* **mruby:** read a reference's identity from the class the bridge registered ([0f24102](https://github.com/elct9620/kobako/commit/0f24102b75f771548cbc39256690e59d6df1f9e4))


### Code Refactoring

* **mruby:** carry a dispatch's block as the block it is ([01d8831](https://github.com/elct9620/kobako/commit/01d8831dff173e80c10d965af8615acb4de2498b))

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.15.0...kobako-mruby-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-mruby:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.14.0...kobako-mruby-v0.15.0) (2026-09-13)


### Features

* **mruby:** upgrade beni to 0.14 and locate a parse failure ([586c910](https://github.com/elct9620/kobako/commit/586c910873710169c0dfce1fe7edb21f9e91cdea))
* **spec:** account for every error scenario the old anchors stated ([179de50](https://github.com/elct9620/kobako/commit/179de5040c54bf3800d3d6ba02b56cf4eaed685b))
* **spec:** declare what the guest answers when a value will not cross ([886a16f](https://github.com/elct9620/kobako/commit/886a16f173efe44766dd9884ac8bfed61113728a))


### Bug Fixes

* **spec:** claim the scenario each test actually witnesses ([ab92e8b](https://github.com/elct9620/kobako/commit/ab92e8b9f5948f16fb86e517b9501598d76e4b89))

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-mruby-v0.13.1...kobako-mruby-v0.14.0) (2026-08-06)


### Features

* **transport:** give a failed Service call a class that says why ([b39f176](https://github.com/elct9620/kobako/commit/b39f1761e9922b7ac761571393f9a6f4478ae522))
* **transport:** let a guest block's exception continue as itself ([c8b1de7](https://github.com/elct9620/kobako/commit/c8b1de7154336fab8fa20c0d7a5b3baaad11d551))


### Bug Fixes

* **transport:** answer a held block failure only to the block that raised ([24dfd6e](https://github.com/elct9620/kobako/commit/24dfd6ec999e0ba1587b9c69da6d2b29563fc875))
* **transport:** spend a block's failure when its Service yields again ([3042a72](https://github.com/elct9620/kobako/commit/3042a72b9e3bb0d6a1d5cb5734d7eaee6a669e85))

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
