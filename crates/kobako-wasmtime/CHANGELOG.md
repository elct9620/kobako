# Changelog

## [0.19.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.18.0...kobako-wasmtime-v0.19.0) (2026-10-26)


### ⚠ BREAKING CHANGES

* **wasmtime:** a struct literal no longer builds the wasmtime driver's Config outside its crate; start from `Config::default()` and assign each field.
* **runtime:** a struct literal no longer builds OpenOptions, Metadata, DirEntry, Snapshot, Capture, or Usage outside kobako-runtime; use their `new` or `Default` and assign each field.
* **wasmtime:** a Service answer past 16 MiB now raises a Service failure in the guest where it used to raise the transport error, and an answer the host cannot write into guest memory now traps the invocation.
* **wasmtime:** a Guest Binary stalling while an invocation sets it up ends in the deadline's trap, and a cap tripped there keeps its own kind, where it used to hang or trap as a generic failure.
* the Guest ABI is version 5, so a Guest Binary built against version 4 is refused at Sandbox construction until rebuilt. In kobako-transport, Reply gains a Break arm and YieldReply::Break carries no payload. The Yielder trait of kobako-runtime gains frame and takes the frame in yield_to_block; the Guest trait of kobako-core and the MrbGuest trait of kobako-mruby take the frame in yield_to_block; Downcall::Yield carries frame and args.
* **io:** putc, IO#read and SystemCallError.new raise RangeError for those numbers instead of writing, refusing or ignoring them.
* **guest:** the default memory_limit is now 4 MiB. JSON.parse in the guest answers an Integer of any width exactly, where it used to answer a Float past 32 bits.
* **guest:** a Service or Extension backend bound at the path File fails its first invocation, naming the segment the Guest Binary now defines.
* **runtime:** the Runtime trait's invoke takes the invocation's Grants as a fourth argument, so every engine implementing it must accept them.

### Features

* check what a rename moves as it moves, refusing a directory beneath a Policy ([23794bf](https://github.com/elct9620/kobako/commit/23794bfcb54e148126e29873fa9b52d9e3de8d8c))
* **guest:** carry mruby-bigint so an Integer has no width of its own ([a74266d](https://github.com/elct9620/kobako/commit/a74266d3e7b1295b75770b62894b232195f9b3bb))
* **guest:** ship File in the default Guest Binary over the filesystem Grants ([535527b](https://github.com/elct9620/kobako/commit/535527b2c6beb82fe37758df62c3804ce5c7191a)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **guest:** widen the guest Integer to 64 bits ([7236a26](https://github.com/elct9620/kobako/commit/7236a26285dfdef175048f659c1094dd3bbb5b4d))
* **io:** create a file only where nothing is through File.open's wx ([56c5665](https://github.com/elct9620/kobako/commit/56c5665e4c87ac30b8035aaed207ce840efad25c))
* **io:** open a file by an Integer mode of File::Constants flags ([e1f484c](https://github.com/elct9620/kobako/commit/e1f484c713df4d894744833836cf9a29ba9677ee))
* **io:** open a file through wasi-libc so a create needs no write ([6d50ca4](https://github.com/elct9620/kobako/commit/6d50ca4d79b70956d113f0798255300aa8a7fe92))
* let a Host App bound how long a Guest Binary takes to be set up ([2addab0](https://github.com/elct9620/kobako/commit/2addab002550539e3ebb24bdd599dd424c2dc3d9))
* **runtime:** hand each invocation its Grants and let an engine declare the kinds it supports ([cba90ee](https://github.com/elct9620/kobako/commit/cba90eed09c8afcb23080cef1c9cbb3650648472)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** keep a directory descriptor's identity its own while open ([6267642](https://github.com/elct9620/kobako/commit/6267642ea831917be76a0101e8d206ccfa8834c1))
* **runtime:** let a dispatch handler abandon its invocation ([179da9e](https://github.com/elct9620/kobako/commit/179da9ee1f31fed9e6d0c149faee0ad55f8db487))
* **runtime:** let a Filesystem's file refuse its close ([af16007](https://github.com/elct9620/kobako/commit/af1600797b098796dd6f329672c7d04e2e1cd2fa)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** let a Mount decide its own access as the one kind of Grant ([5c9189e](https://github.com/elct9620/kobako/commit/5c9189eea8ac61cf165dca1b38e459c4f14acdd0)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **runtime:** let a Mount's Policy judge the spelling the host stores ([5281057](https://github.com/elct9620/kobako/commit/52810573f22d2a3edfae9b34b7767a5f440fba15))
* **runtime:** let the records a Filesystem or an engine builds gain fields ([c0ef88d](https://github.com/elct9620/kobako/commit/c0ef88d0b6df16d651e4c49226e41667307d772f))
* **runtime:** name a file grown past what a Filesystem keeps ([7bac3e6](https://github.com/elct9620/kobako/commit/7bac3e65534097b85d93f5967b6c73a979869a78))
* **runtime:** name each code a Filesystem may fail with ([1d980fa](https://github.com/elct9620/kobako/commit/1d980fa4328bad42e8a8665adfec9fc844690e97))
* **runtime:** refuse a Mount at a path that is no absolute guest path ([a525758](https://github.com/elct9620/kobako/commit/a5257589f9d831b9a3b6d1952df6e5c8469630e1))
* **wasmtime:** answer a Filesystem out of space as ENOSPC ([c648ebf](https://github.com/elct9620/kobako/commit/c648ebf63a871b7d4417110de00decf1738a19d6))
* **wasmtime:** cap the descriptors one invocation holds open ([03409c1](https://github.com/elct9620/kobako/commit/03409c1437f71312c584a92e22054b90241535a3))
* **wasmtime:** let Config gain a cap without breaking a frontend ([55d8093](https://github.com/elct9620/kobako/commit/55d80933b958bab67f1b94deb7633272dc56f972))
* **wasmtime:** present each filesystem Grant to the guest as a preopen ([68149a7](https://github.com/elct9620/kobako/commit/68149a7adc912172b72e7a34bd39082a3b2a9e95)), closes [#78](https://github.com/elct9620/kobako/issues/78)


### Bug Fixes

* append through a Filesystem's file only in one step ([0509ca7](https://github.com/elct9620/kobako/commit/0509ca70d2d5a832303973ba0f7280be0731fd3d))
* ask a mount's block once for a read ([f89bd16](https://github.com/elct9620/kobako/commit/f89bd16c1a71ed1d385ab9d3ac5974ac3f0a9103))
* follow no link swapped in after a Policy judges a path ([ed93213](https://github.com/elct9620/kobako/commit/ed9321303dd45e113fc413e99e1240118cc4bbe4))
* hold an Overlay's lock across each call, waiting without the GVL ([b8e7342](https://github.com/elct9620/kobako/commit/b8e734273fdc577a40be97c43c46d9548d66c625))
* **io:** build Errno and IOError as CRuby does ([da77c47](https://github.com/elct9620/kobako/commit/da77c4725d3701ba982f5425b1fcf2fb0038ac89))
* **io:** give File::Constants CRuby's Linux values ([14cadd8](https://github.com/elct9620/kobako/commit/14cadd88ac5a5dcb8f73b252b68c493488b3e78a))
* **io:** keep the number a SystemCallError is built with ([5fd327a](https://github.com/elct9620/kobako/commit/5fd327add59173cd5756fecb41d97aebcba7b881))
* **io:** raise each code the engine answers as its own Errno ([eb6382e](https://github.com/elct9620/kobako/commit/eb6382e506a30e82576fa12ab9859e7d35be2b17))
* **io:** raise Errno::EIO for a file failure no other code names ([8a0447d](https://github.com/elct9620/kobako/commit/8a0447d2d09ea83e6f7771cf0411bfd6deec3d1a)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **io:** raise RangeError for a number past what putc, read and errno take ([6f87cc0](https://github.com/elct9620/kobako/commit/6f87cc08dff7146415f80ca25cd75ddbec4391c0))
* **io:** raise the error a failed file write names ([bcc8e00](https://github.com/elct9620/kobako/commit/bcc8e00662741fcac3b91f58c0b60f4a79d2ab9f))
* **io:** write a file to the end when its Filesystem takes it in parts ([029b439](https://github.com/elct9620/kobako/commit/029b43939a3a48bc2925057df69bfcb85b1a0548))
* keep a link within the directory a descriptor opened ([952dd05](https://github.com/elct9620/kobako/commit/952dd0556969683f44798da94f9fc7a17c1f6d55))
* run a stored block as the block of the Call that carried it ([bf33dc1](https://github.com/elct9620/kobako/commit/bf33dc1fd40c83498c6c12b0be52b3a835a144cf))
* **runtime:** make every Filesystem answer the methods that keep a guarantee ([00f5af6](https://github.com/elct9620/kobako/commit/00f5af63f34ac7722249746d0b0eb566da618803))
* **runtime:** report a merged directory's identity alike through an Overlay's lstat ([ded3b4a](https://github.com/elct9620/kobako/commit/ded3b4aba78efa678318c38b139b5c9c76b5d3fd))
* **wasmtime:** answer a Call the host cannot serve with the reason ([cf60dcc](https://github.com/elct9620/kobako/commit/cf60dccc63a0d0cfc959f701b7212c78beafa528))
* **wasmtime:** answer a path past Linux's limits as NAMETOOLONG ([af43917](https://github.com/elct9620/kobako/commit/af43917e30a634d6c1bd70813044967bd1057ba3))
* **wasmtime:** answer a stat that follows no link with the link itself ([ad0072e](https://github.com/elct9620/kobako/commit/ad0072e03ecc363487598a507de20a8f7aec2927))
* **wasmtime:** answer a wait to write a file by what its Mount permits ([bf863db](https://github.com/elct9620/kobako/commit/bf863db998edeab60f9276fc53e8adcf8cf98364))
* **wasmtime:** answer an offset past the largest signed offset as EINVAL ([b94788e](https://github.com/elct9620/kobako/commit/b94788e1c1fc9d86f647da4396bcba3eaec16242))
* **wasmtime:** answer an unlink with a trailing slash by the name itself ([94e7250](https://github.com/elct9620/kobako/commit/94e72505cfc027319c2139e15488eb2682268d49))
* **wasmtime:** answer descriptor flags, advice, and clock waits as preview1 does ([c1aec63](https://github.com/elct9620/kobako/commit/c1aec632445c3bf216be6375552c799d7877670f))
* **wasmtime:** answer dot paths, creates, and far seeks as Linux does ([68c234e](https://github.com/elct9620/kobako/commit/68c234e06859b93fb986bdbab92cca944ce39700))
* **wasmtime:** answer EINVAL for a guest path holding a NUL ([1c681d6](https://github.com/elct9620/kobako/commit/1c681d6e723f259ab2b3acc5e3a33d85499a2ef5))
* **wasmtime:** answer ENOENT for an open of a missing path that makes nothing ([48e3944](https://github.com/elct9620/kobako/commit/48e3944c64d7ef410fcc4595d80ba0284f257a09))
* **wasmtime:** answer every preview1 call that reads a descriptor from kobako's own table ([d0e39ae](https://github.com/elct9620/kobako/commit/d0e39aebd6d729d49111340af7f2af4e37aa7909)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **wasmtime:** answer fd_allocate beneath a Mount as unsupported ([e3d2071](https://github.com/elct9620/kobako/commit/e3d20713e4474cd637dacfece99e52b80d7ad2f6))
* **wasmtime:** answer what stands at a path before a Mount refuses an open ([81e02dc](https://github.com/elct9620/kobako/commit/81e02dc5e7d500b2971ab5d4a670d38601f89389))
* **wasmtime:** ask a Policy once for an open that may create ([a4613cb](https://github.com/elct9620/kobako/commit/a4613cb8cff89af51f55ce090433c61268105789))
* **wasmtime:** bound a buffer outside memory before a file's access is checked ([8b76f76](https://github.com/elct9620/kobako/commit/8b76f764a6336d2e77baa2ca6fe63dfd7663ce19))
* **wasmtime:** bound reads, writable waits, and a FIFO's names as preview1 hosts do ([6d89e55](https://github.com/elct9620/kobako/commit/6d89e55541f3366815dd7ed271e813a6ba41bcb3))
* **wasmtime:** bound what one preview1 call copies to wasmtime's hostcall fuel ([097c554](https://github.com/elct9620/kobako/commit/097c55426c1e71467bb1e372abc190331dd3e601))
* **wasmtime:** check path_open's flags as wasmtime-wasi does ([22eb494](https://github.com/elct9620/kobako/commit/22eb4944117260831e92fe616b02888078c02b0c))
* **wasmtime:** close the file fd_renumber moves a descriptor onto ([0f6148d](https://github.com/elct9620/kobako/commit/0f6148d536ed3e9e96749c2e4a60bf6275332655))
* **wasmtime:** give each writer of a cached artifact its own temp file ([995a46e](https://github.com/elct9620/kobako/commit/995a46e99dc1c485a303bb7f023b4e668a2801a3))
* **wasmtime:** grant the compile instance no ambient authority ([c3edc71](https://github.com/elct9620/kobako/commit/c3edc71ab99dac57cbabfc685a7cc60a5a5c06ac))
* **wasmtime:** hold the whole invocation to its deadline ([231e325](https://github.com/elct9620/kobako/commit/231e32592699008adf3105050bee8b5528ea27a8))
* **wasmtime:** judge the cache directory itself, not where a link leads ([6baf6fd](https://github.com/elct9620/kobako/commit/6baf6fd2ad845cea83049c4b2bb51174aa205cd0))
* **wasmtime:** keep a write and its file position within the largest signed offset ([29d4c34](https://github.com/elct9620/kobako/commit/29d4c341fc5a47ee43fdd249918e1f3e68120f86))
* **wasmtime:** key the compiled-artifact cache by the engine that compiles it ([b43bad2](https://github.com/elct9620/kobako/commit/b43bad2fe176c153ed82a3a8825698414df78dfa))
* **wasmtime:** leave a link named with a trailing slash in place ([9b3cdea](https://github.com/elct9620/kobako/commit/9b3cdea6dc00353c5c4896ec29b88eebdaddc8ab))
* **wasmtime:** list a directory once for each listing a guest reads ([c333001](https://github.com/elct9620/kobako/commit/c3330011725c3dba01ea248ed7d1dfea31085af0))
* **wasmtime:** look up what stands before a dot path as Linux does ([0898eaf](https://github.com/elct9620/kobako/commit/0898eafb55397c84ca9007561ddb35dd007ed2b8))
* **wasmtime:** place an append at the end as the file stands when it lands ([d7291c4](https://github.com/elct9620/kobako/commit/d7291c4f32fb1f5f2494c69c76796476bbcf4d43))
* **wasmtime:** reach the artifact cache through the directory it checked ([462fe91](https://github.com/elct9620/kobako/commit/462fe918d516cd639e405b06a5e280a9e58624df))
* **wasmtime:** refuse a `..` that steps out of a host link ([4599386](https://github.com/elct9620/kobako/commit/45993868c7334376f5d1a455afbd15fe4a2146bc))
* **wasmtime:** refuse a directory made in an opened directory's place ([5e67d4c](https://github.com/elct9620/kobako/commit/5e67d4cd2a52e3aa37bdd615fe1ae101a618db88))
* **wasmtime:** refuse a link an open asks not to follow ([123a01b](https://github.com/elct9620/kobako/commit/123a01b01f59cc69c0e3a23f9aeb01aa89132c07))
* **wasmtime:** refuse a link beneath a Policy before the Policy judges the path ([0e08e7b](https://github.com/elct9620/kobako/commit/0e08e7b463344ae34b5feb1787f8af8a6c719063))
* **wasmtime:** refuse a link to a directory an open asks not to follow ([eec218f](https://github.com/elct9620/kobako/commit/eec218fdd5584f641070032cf1110ef1683eaae5))
* **wasmtime:** refuse a path climbing above the directory it is opened through ([ae52746](https://github.com/elct9620/kobako/commit/ae52746761624249b7ff1e1fa09ece4ea8130ced))
* **wasmtime:** refuse a path through a link beneath a Mount with a Policy ([3da1641](https://github.com/elct9620/kobako/commit/3da16416692ade01db2d9cf366d54d3ec5f50605))
* **wasmtime:** refuse a rename of a file onto a path ending in a slash ([486a0ab](https://github.com/elct9620/kobako/commit/486a0ab3584772b58e181d5de57ef505e610eb78))
* **wasmtime:** refuse an argument preview1 cannot hold in wasmtime-wasi's order ([9c76e21](https://github.com/elct9620/kobako/commit/9c76e2181a0dabd81f02723154cc4290215fcf70))
* **wasmtime:** refuse to remove or move a Mount's root ([9f2e000](https://github.com/elct9620/kobako/commit/9f2e000115a4dd4aecfcc4b1cd39b23f6f20fe05))
* **wasmtime:** report a host FIFO or device as of an unknown filetype ([5a212e7](https://github.com/elct9620/kobako/commit/5a212e7a7c0f069ffdcc56b850fab3778863d9ee))
* **wasmtime:** require wasmtime 49.0.4 for the aarch64 br_table fix ([390e30b](https://github.com/elct9620/kobako/commit/390e30ba51ed4175820f2bb230e3ad5b0ad1e9a5))
* **wasmtime:** resolve a `..` and a trailing slash as a host does ([d2d04c1](https://github.com/elct9620/kobako/commit/d2d04c1fff24d8829b2cf4fcbf628565f35eeadc))
* **wasmtime:** resolve a final dot through the name before it ([7a0d3d8](https://github.com/elct9620/kobako/commit/7a0d3d8b23d47d4ba5c942682841434e74d6bf6a))
* **wasmtime:** resolve a link a stat names with a trailing slash ([6da98fa](https://github.com/elct9620/kobako/commit/6da98facbccaa9bf6d8f8280a56841e19a7c879b))
* **wasmtime:** size a file opened only to read as wasmtime-wasi and Linux answer ([810f5c8](https://github.com/elct9620/kobako/commit/810f5c8e863dc5bdbe82f8a19c7559fd7eee03f9))
* **wasmtime:** sync a cached artifact before renaming it into place ([097efc7](https://github.com/elct9620/kobako/commit/097efc77599545bb2a5e729c75d23082470ed8f4))
* **wasmtime:** take wasmtime 49.0.3 for its sandbox advisories ([7672592](https://github.com/elct9620/kobako/commit/76725923cac50473c7e3a21cc5121e3cbba0cba5))
* **wasmtime:** trap a guest field address past the address space ([de5d47e](https://github.com/elct9620/kobako/commit/de5d47e10aa5d978573e46a7cb3453ff248d006e))


### Performance Improvements

* **io:** answer IO#read with a length in one read the file holds ([e9161a2](https://github.com/elct9620/kobako/commit/e9161a2018c1617157025de275f77890e865b7d1)), closes [#78](https://github.com/elct9620/kobako/issues/78)

## [0.18.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.17.0...kobako-wasmtime-v0.18.0) (2026-10-13)


### ⚠ BREAKING CHANGES

* the kobako and kobako-wasmtime crates declare a minimum supported Rust version of 1.96.
* **transport:** the stdin layout a Guest Binary reads changed within ABI 4. kobako_transport::envelope::Frames is replaced by Preamble, which is #[non_exhaustive] and built from Default; Bindings and Snippets no longer have their own decode and encode. Guest::eval and Guest::run, MrbGuest::eval and MrbGuest::run, and Runtime::invoke take a Preamble in place of Frames. kobako_core re-exports Preamble and Flag in place of Frames.
* **transport:** the ABI version is 4, so a Guest Binary built for ABI 3 is refused at Sandbox construction. kobako_core::export_guest! emits __kobako_downcall in place of __kobako_eval, __kobako_run, __kobako_yield_to_block and __kobako_take_outcome, and the host import is __kobako_upcall. kobako_core::abi::{take_outcome, write_outcome, write_reply, decode_run} are replaced by serve and answer, and the frames module is no longer public. Reply, YieldReply and Outcome gain an Unsupported arm and take their tags from the shared registry, and kobako_core::DispatchError and kobako::YieldError gain an Unsupported arm. A yield to a guest without block support raises Kobako::UnsupportedError at the Service's yield instead of failing the invocation as a trap.
* **runtime:** Runtime::invoke takes &kobako_transport::envelope::Frames in place of kobako_runtime::Frames, whose byte fields are gone. Guest::eval and Guest::run (and their MrbGuest flows) receive the decoded Frames, eval also its source; kobako_core::frames::read_frame is replaced by read_eval_frames and read_frames.
* **runtime:** kobako_runtime::snapshot::Completion::Outcome holds a decoded kobako_transport::envelope::Outcome instead of Vec<u8>; an engine decodes the result itself and reports an unframed one as
* **runtime:** kobako_runtime::runtime::Entry::Run holds { entrypoint: &str, payload: Vec<u8> } instead of { envelope: &[u8] }; a frontend passes the fields, and an engine encodes the Run envelope itself.
* **runtime:** kobako_runtime::yielder::Yielder::yield_to_block returns Result<YieldReply, Trap> instead of the encoded bytes; an engine implementing it decodes the reply itself.
* a memory budget or deadline reached during a Service answer or a yielded block now raises Kobako::MemoryLimitError or

### Features

* **guest:** raise Kobako::UnsupportedError when a host does not serve a call ([db003af](https://github.com/elct9620/kobako/commit/db003af75cf2d11b5f644986aa77c0408f5f59ba)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** compile a Preamble's snippets on an instance of their own ([303432d](https://github.com/elct9620/kobako/commit/303432dd90ed62dc378391515e68e784e1b473a4)), closes [#75](https://github.com/elct9620/kobako/issues/75)
* **transport:** carry each operation as a tag on one channel per direction ([c7d574b](https://github.com/elct9620/kobako/commit/c7d574b59a42e58632f2bdc46cc9a8e4c7ac6121)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** keep an unknown request's body, and refuse tag 0x00 ([edb64df](https://github.com/elct9620/kobako/commit/edb64df8f87694d17c5b6fd71ac63329023306ec)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **transport:** set an invocation up from a Preamble of tagged records ([84d4498](https://github.com/elct9620/kobako/commit/84d4498464f94d7312f26931ddd4ec43894a6654))


### Bug Fixes

* end the invocation as the trap a host callback into the guest hits ([b8f47d3](https://github.com/elct9620/kobako/commit/b8f47d3699a19adcfd5596af93136401a286c761))
* keep a compile's bytecode only when the Preamble still fits it ([765db08](https://github.com/elct9620/kobako/commit/765db08ad31c8f5163794c7e144355f3237af93b))
* **wasmtime:** keep a compile answer only when it covers every source snippet ([93e9ddd](https://github.com/elct9620/kobako/commit/93e9ddd79f714446ae713a3145c59d3d2f4145dc))
* **wasmtime:** tell every frontend how to recover, not only the gem ([cab805a](https://github.com/elct9620/kobako/commit/cab805ab62441a0ca2c78775b7a0d7ce9cc05c43))
* **wasmtime:** word a missing or mistyped export one way ([8ab0aee](https://github.com/elct9620/kobako/commit/8ab0aeec8bda2960b6cab0502ef51860ea9093b8))


### Code Refactoring

* **runtime:** hand a frontend the decoded Outcome ([0da6aac](https://github.com/elct9620/kobako/commit/0da6aac0f7943d694a5a4ab9439b2d6c527ae1b0)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** hand a frontend the decoded Yield Reply ([e80ab54](https://github.com/elct9620/kobako/commit/e80ab54e36c2a80b0f746c675f85bd1238fd4331)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** let the ABI endpoints frame and read the Frames ([dea069a](https://github.com/elct9620/kobako/commit/dea069ac49a59c12a6f7102ed3cf2925f316cbff)), closes [#73](https://github.com/elct9620/kobako/issues/73)
* **runtime:** let the engine frame a run's entrypoint and payload ([62f9c94](https://github.com/elct9620/kobako/commit/62f9c9407ff3ce690723625bea075fa49d2dda47)), closes [#73](https://github.com/elct9620/kobako/issues/73)


### Build System

* declare the Rust 1.96 floor wasmtime 49 sets ([f49a2df](https://github.com/elct9620/kobako/commit/f49a2df88fa51d3b0eecb619cab1e98d79bb4245))

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.16.0...kobako-wasmtime-v0.17.0) (2026-09-20)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.15.0...kobako-wasmtime-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.14.0...kobako-wasmtime-v0.15.0) (2026-09-13)


### Features

* **spec:** bring the Rust frontend under the behaviors it answers ([b62243f](https://github.com/elct9620/kobako/commit/b62243f68b338fd219136d60c9cf760d0be03fc0))
* **spec:** declare the bytes both sides must already agree on ([3875755](https://github.com/elct9620/kobako/commit/3875755999550511837808dcd24a25086bedc926))
* **spec:** declare what the driver holds a run to ([4539cc1](https://github.com/elct9620/kobako/commit/4539cc16a7ddb6c1706c36282e64182bb480b684))

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.13.1...kobako-wasmtime-v0.14.0) (2026-08-06)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.13.0...kobako-wasmtime-v0.13.1) (2026-07-30)


### Bug Fixes

* **host:** keep nothing in the ABI probe's capture pipes ([a3d6c1c](https://github.com/elct9620/kobako/commit/a3d6c1c8edb9964a7d8ab162b2e379b39dbbddb9))


### Performance Improvements

* **host:** ask an artifact its ABI version once, not once per Sandbox ([693b94f](https://github.com/elct9620/kobako/commit/693b94f878e56815fe3faeae94e5c4d0cd9c8bb6))

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.12.0...kobako-wasmtime-v0.13.0) (2026-07-29)


### ⚠ BREAKING CHANGES

* **wasmtime:** put every cap in one struct, named the way the field is read
* **runtime:** give the engine contract names an implementer can write
* **transport:** name the core envelope's types after what they carry
* **wire:** route every tier through the one envelope
* **wire:** carry dispatch over the core envelope with an opaque payload

### Features

* **wire:** carry dispatch over the core envelope with an opaque payload ([556104b](https://github.com/elct9620/kobako/commit/556104bf86fdf481b5368b70af83eb0add4b2708))


### Code Refactoring

* **runtime:** give the engine contract names an implementer can write ([0a41491](https://github.com/elct9620/kobako/commit/0a41491360cf17def8245c0758679e92572b990f))
* **transport:** name the core envelope's types after what they carry ([b44de65](https://github.com/elct9620/kobako/commit/b44de6502953826c567d4ef8a594561479f50c1d))
* **wasmtime:** put every cap in one struct, named the way the field is read ([3f57860](https://github.com/elct9620/kobako/commit/3f578608b044d1ade0ae35231266cf3c9e517c02))
* **wire:** route every tier through the one envelope ([c5cd33a](https://github.com/elct9620/kobako/commit/c5cd33a5346f49857fa6e1f45c9cf9b9bea0ff77))

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.11.0...kobako-wasmtime-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.10.2...kobako-wasmtime-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.10.1...kobako-wasmtime-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.10.0...kobako-wasmtime-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.9.0...kobako-wasmtime-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.8.0...kobako-wasmtime-v0.9.0) (2026-07-11)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.7.0...kobako-wasmtime-v0.8.0) (2026-07-08)


### Bug Fixes

* **crates:** keep the no-timeout epoch deadline within range ([a3255df](https://github.com/elct9620/kobako/commit/a3255df98a77825bb39b571a60fe6ff83d269d19))

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.6.1...kobako-wasmtime-v0.7.0) (2026-07-03)


### Features

* **crates:** build the requested isolation profile into the WASI context ([63c25d8](https://github.com/elct9620/kobako/commit/63c25d835d4d03010c1658217cee412318e6b5d8))
* **runtime:** runtimes declare their isolation profile ([f89717a](https://github.com/elct9620/kobako/commit/f89717a6bc9809f0de0df78f97da33a49a1474ac))

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-wasmtime-v0.6.0...kobako-wasmtime-v0.6.1) (2026-07-02)


### Miscellaneous Chores

* **kobako-wasmtime:** Synchronize kobako crates versions
