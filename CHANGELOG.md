# Changelog

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
