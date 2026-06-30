# Changelog

## [0.12.2](https://github.com/elct9620/kobako/compare/v0.12.1...v0.12.2) (2026-06-30)


### Bug Fixes

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
