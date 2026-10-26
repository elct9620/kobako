# Changelog

## [0.19.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.18.0...kobako-io-v0.19.0) (2026-10-26)


### ⚠ BREAKING CHANGES

* **io:** IO#seek in the guest raises Errno::EINVAL instead of ArgumentError for a negative offset from the start or an unknown whence.
* **io:** File.open raises ArgumentError for a mode such as bw, where b leads, or wbt, holding both b and t, as CRuby does, instead of opening the file.
* **io:** putc, IO#read and SystemCallError.new raise RangeError for those numbers instead of writing, refusing or ignoring them.
* **guest:** the kobako guest crates name beni 0.25's types, so a shell or codec built on them moves to beni 0.25 with them.
* **guest:** the default memory_limit is now 4 MiB. JSON.parse in the guest answers an Integer for every integer within 64 bits, where it used to answer a Float past 32 bits. An integer past 64 bits now raises
* **guest:** the kobako guest crates name beni 0.24's types, so a shell or codec built on them moves to beni 0.24 with them.
* **io:** reading `IO#sync` after assigning a non-boolean now answers true or false. It used to answer the assigned value, and nil used to read back as true.
* **io:** a script writing to a file through `IO#write` beneath a Mount now raises on a failed write where it used to go on.
* **io:** a script rescuing IOError for a failed read or write beneath a Mount must rescue the EIO error class, or SystemCallError, instead.
* **guest:** a Service or Extension backend bound at the path File fails its first invocation, naming the segment the Guest Binary now defines.

### Features

* **guest:** ship File in the default Guest Binary over the filesystem Grants ([535527b](https://github.com/elct9620/kobako/commit/535527b2c6beb82fe37758df62c3804ce5c7191a)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **guest:** widen the guest Integer to 64 bits ([7236a26](https://github.com/elct9620/kobako/commit/7236a26285dfdef175048f659c1094dd3bbb5b4d))
* **io:** create a file only where nothing is through File.open's wx ([56c5665](https://github.com/elct9620/kobako/commit/56c5665e4c87ac30b8035aaed207ce840efad25c))
* **io:** let IO.new take its descriptor's access when no mode is given ([77669cc](https://github.com/elct9620/kobako/commit/77669ccd958ea9518e69b50670f2804d66c9e1c5))
* **io:** name a call that would deadlock Errno::EDEADLK ([8a090e6](https://github.com/elct9620/kobako/commit/8a090e6f6949ec8019743d903c712e122d7743f9))
* **io:** name a full descriptor table Errno::EMFILE ([67876a7](https://github.com/elct9620/kobako/commit/67876a78764f8afe1b11167471d3d94e81c92af1))
* **io:** name a move across devices Errno::EXDEV ([dbed1ce](https://github.com/elct9620/kobako/commit/dbed1ce20eed6c6f1b6715f70431f0f6a7012e9e))
* **io:** name a path past its limits Errno::ENAMETOOLONG ([7679a6f](https://github.com/elct9620/kobako/commit/7679a6feb0fce0b00c89f36ed33bd5d872f86abe))
* **io:** open a file by an Integer mode of File::Constants flags ([e1f484c](https://github.com/elct9620/kobako/commit/e1f484c713df4d894744833836cf9a29ba9677ee))
* **io:** open a file through wasi-libc so a create needs no write ([6d50ca4](https://github.com/elct9620/kobako/commit/6d50ca4d79b70956d113f0798255300aa8a7fe92))
* **io:** raise Errno::EFBIG for a file grown too large ([b8ea9cd](https://github.com/elct9620/kobako/commit/b8ea9cdb6ecab7adae7f5487cffdb57889bfbbb5))
* **io:** set a file's position with IO#pos= ([1802d5a](https://github.com/elct9620/kobako/commit/1802d5a32618f33cc189e24f0de4dbfda074e3db))
* **runtime:** let a Filesystem's file refuse its close ([af16007](https://github.com/elct9620/kobako/commit/af1600797b098796dd6f329672c7d04e2e1cd2fa)), closes [#78](https://github.com/elct9620/kobako/issues/78)


### Bug Fixes

* **io:** build Errno and IOError as CRuby does ([da77c47](https://github.com/elct9620/kobako/commit/da77c4725d3701ba982f5425b1fcf2fb0038ac89))
* **io:** carry a stream's descriptor in its payload, not in hidden ivars ([76ca3c0](https://github.com/elct9620/kobako/commit/76ca3c03fdef537041a167bb58b1af2b176d772a))
* **io:** give File::Constants CRuby's Linux values ([14cadd8](https://github.com/elct9620/kobako/commit/14cadd88ac5a5dcb8f73b252b68c493488b3e78a))
* **io:** keep a stream's bookkeeping out of its instance variables ([5ecf833](https://github.com/elct9620/kobako/commit/5ecf8332c2ebff9a1d01c528fa3af31afdebdee3))
* **io:** keep the number a SystemCallError is built with ([5fd327a](https://github.com/elct9620/kobako/commit/5fd327add59173cd5756fecb41d97aebcba7b881))
* **io:** raise each code the engine answers as its own Errno ([eb6382e](https://github.com/elct9620/kobako/commit/eb6382e506a30e82576fa12ab9859e7d35be2b17))
* **io:** raise Errno::EIO for a file failure no other code names ([8a0447d](https://github.com/elct9620/kobako/commit/8a0447d2d09ea83e6f7771cf0411bfd6deec3d1a)), closes [#78](https://github.com/elct9620/kobako/issues/78)
* **io:** raise RangeError for a number past what putc, read and errno take ([6f87cc0](https://github.com/elct9620/kobako/commit/6f87cc08dff7146415f80ca25cd75ddbec4391c0))
* **io:** raise the error a failed file write names ([bcc8e00](https://github.com/elct9620/kobako/commit/bcc8e00662741fcac3b91f58c0b60f4a79d2ab9f))
* **io:** read a directory as EISDIR and wrap only an access a file holds ([df26095](https://github.com/elct9620/kobako/commit/df26095c681704cef0a8249c8b30797957fb8bd0))
* **io:** read IO.new's mode over a guest file as File.open does ([db3402c](https://github.com/elct9620/kobako/commit/db3402cd5c9c67ae48fb0363e1153a76acc55c17))
* **io:** refuse a seek before the start or by an unknown whence as EINVAL ([2f4906c](https://github.com/elct9620/kobako/commit/2f4906cc0ad3a3aa9613789e7d63c66ce881883f))
* **io:** write a file to the end when its Filesystem takes it in parts ([029b439](https://github.com/elct9620/kobako/commit/029b43939a3a48bc2925057df69bfcb85b1a0548))


### Performance Improvements

* **io:** answer IO#read with a length in one read the file holds ([e9161a2](https://github.com/elct9620/kobako/commit/e9161a2018c1617157025de275f77890e865b7d1)), closes [#78](https://github.com/elct9620/kobako/issues/78)


### Build System

* **guest:** rebuild the guest crates on beni 0.24 ([4599337](https://github.com/elct9620/kobako/commit/4599337261d1d28ff8c36435062d34cbccf09abb))
* **guest:** rebuild the guest crates on beni 0.25 ([a06dfd2](https://github.com/elct9620/kobako/commit/a06dfd25958ded9c82072149d73e56372343ba31))

## [0.18.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.17.0...kobako-io-v0.18.0) (2026-10-13)


### ⚠ BREAKING CHANGES

* **guest:** the kobako guest crates name beni 0.23's types, so a shell or codec built on them moves to beni 0.23 with them.
* **io:** IO.new raises TypeError for a mode that is not a String, such as :w.
* **guest:** the kobako guest crates name beni 0.22's types, so a shell or codec built on them moves to beni 0.22 with them.
* **guest:** the kobako-mruby harness names beni 0.21's types — the keyword Hash in `Arguments` and `PayloadCodec::encode_call_arguments` is `beni::RHash` — so a shell or codec built on these crates moves to beni 0.21 with them.

### Features

* **guest:** rebuild the guest crates on beni 0.21 ([8ec00ff](https://github.com/elct9620/kobako/commit/8ec00ff94ec21b6bbfe8b88e0a4e75f4489dd692))


### Bug Fixes

* **io:** bracket each argument IO#write converts in an arena scope ([c1ef1b7](https://github.com/elct9620/kobako/commit/c1ef1b794bddda9347bf40436646cae8bccdd227))
* **io:** read IO.new's mode only as a String ([08954ec](https://github.com/elct9620/kobako/commit/08954ec8b14857be1f7ab10df6132e07577f802a))


### Build System

* **guest:** rebuild the guest crates on beni 0.22 ([fd6aeb9](https://github.com/elct9620/kobako/commit/fd6aeb92705fa53c52e2cd75b4918a58baa6637e))
* **guest:** rebuild the guest crates on beni 0.23 ([a6782da](https://github.com/elct9620/kobako/commit/a6782dad4c2c68a25573f82a8a16588c09856d23))

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.16.0...kobako-io-v0.17.0) (2026-09-20)


### ⚠ BREAKING CHANGES

* **guest:** a pattern's allocator is undefined from the moment the capability installs rather than from its first wrap, so allocating one is refused for the missing allocator throughout an invocation, where before the first statement could still reach an uninitialized carrier.
* **guest:** `Kobako::raise_transport_error`, `raise_service_error` and `reraise` are gone. A flow of its own builds `Kobako::transport_error` or `service_error` and hands the result back as `Err`, which beni raises at the guest call site.

### Features

* **guest:** rebuild the guest crates on beni 0.17 ([b397701](https://github.com/elct9620/kobako/commit/b397701231c7d8718503d24f73b175676d0d1be3))
* **guest:** rebuild the guest crates on beni 0.18 ([ae232eb](https://github.com/elct9620/kobako/commit/ae232ebbf708419ff0672172fbf8627363fb02da))

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.15.0...kobako-io-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.14.0...kobako-io-v0.15.0) (2026-09-13)


### Features

* **mruby:** upgrade beni to 0.14 and locate a parse failure ([586c910](https://github.com/elct9620/kobako/commit/586c910873710169c0dfce1fe7edb21f9e91cdea))

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.13.1...kobako-io-v0.14.0) (2026-08-06)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.13.0...kobako-io-v0.13.1) (2026-07-30)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.12.0...kobako-io-v0.13.0) (2026-07-29)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.11.0...kobako-io-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.10.2...kobako-io-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* release the guest crates at 0.11.0 ([83391c1](https://github.com/elct9620/kobako/commit/83391c15a2bd7b162495e851ad1603a047b0cf0e))

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-io-v0.10.1...kobako-io-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.10.0...kobako-io-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.9.0...kobako-io-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.8.0...kobako-io-v0.9.0) (2026-07-11)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.7.0...kobako-io-v0.8.0) (2026-07-08)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.6.1...kobako-io-v0.7.0) (2026-07-03)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako crates versions

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.6.0...kobako-io-v0.6.1) (2026-07-02)


### Bug Fixes

* **guest:** size collection conversions by C array length, not #length ([90ecbd0](https://github.com/elct9620/kobako/commit/90ecbd0cb6a990b8c5a1e5deec3a10df4eaa37df))
* **io:** enforce the fd allowlist at the write syscall ([1b300df](https://github.com/elct9620/kobako/commit/1b300df7bee8f87b701f76b42300163a8899b93e))

## [0.6.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.5.2...kobako-io-v0.6.0) (2026-06-26)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## [0.5.2](https://github.com/elct9620/kobako/compare/kobako-io-v0.5.1...kobako-io-v0.5.2) (2026-06-24)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## [0.5.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.5.0...kobako-io-v0.5.1) (2026-06-14)


### Bug Fixes

* **guest:** adopt beni 0.7.0 protected dispatch (B-51) ([c61655b](https://github.com/elct9620/kobako/commit/c61655bcead336d32a4b6ff7ff1b34c21cdfccd9))

## [0.5.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.4.1...kobako-io-v0.5.0) (2026-06-12)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## [0.4.1](https://github.com/elct9620/kobako/compare/kobako-io-v0.4.0...kobako-io-v0.4.1) (2026-06-11)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## [0.4.0](https://github.com/elct9620/kobako/compare/kobako-io-v0.3.0...kobako-io-v0.4.0) (2026-06-10)


### Miscellaneous Chores

* **kobako-io:** Synchronize kobako guest crates versions

## 0.3.0 (2026-06-08)


### Miscellaneous Chores

* release the guest crates at 0.3.0 ([27a0997](https://github.com/elct9620/kobako/commit/27a099766404cd9c32c54b334dc76d8ec1827675))
