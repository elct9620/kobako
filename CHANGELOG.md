# Changelog

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
