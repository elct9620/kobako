# Changelog

## [0.18.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.17.0...kobako-regexp-v0.18.0) (2026-10-10)


### ⚠ BREAKING CHANGES

* **regexp:** Regexp.escape and Regexp.quote raise ArgumentError for more than one argument, and a substitution result whose to_s is not a String is written as its default description instead of raising.
* **regexp:** `=~` on a receiver other than String, Regexp, Symbol or nil raises NoMethodError instead of answering nil, and a Symbol on the left now matches its name instead of answering nil.
* **guest:** the kobako guest crates name beni 0.22's types, so a shell or codec built on them moves to beni 0.22 with them.
* **regexp:** Regexp#match, #match?, #=~, Regexp.escape and the MatchData group readers read a Symbol argument by its own name, where they used to read what a redefined Symbol#to_s answered. Pass the rendered text as a String to keep the old reading.
* **regexp:** Regexp, String and MatchData methods of the regexp capability read a String argument or receiver as its own characters, where they used to read what a redefined String#to_s answered. Pass the rendered text explicitly to keep the old reading.
* **regexp:** MatchData#begin, #end and #offset raise TypeError for a group that is neither a number nor a name, and read a Float as its whole part, where they used to answer for the whole match. Pass 0 to ask for the whole match.
* **regexp:** Regexp.new and Regexp.compile raise ArgumentError for a flag string carrying a letter other than i, m or x, where they used to ignore it. Drop the unknown letters from the flag string.
* **regexp:** Regexp.new and Regexp.compile read an option that is neither an Integer nor a String by its truth, giving IGNORECASE or no option, where they used to read the letters of its #to_s. Pass the option as a String ("m") or an Integer (Regexp::MULTILINE) instead.
* **regexp:** Regexp.escape and Regexp.quote raise TypeError for a value that is neither a String nor a Symbol, where they used to escape
* **regexp:** Regexp.new and Regexp.compile given a Regexp answer that pattern's own source and options, where they used to answer its inline-flag rendering with no options; options passed alongside are ignored. Pass regexp.to_s to keep the old reading.
* **regexp:** Regexp.new and Regexp.compile raise TypeError for a source that is neither a String nor a Regexp, where they used to compile its #to_s. Pass source.to_s instead.
* **regexp:** String#sub and #gsub raise TypeError for a replacement that is neither a String nor a Hash, where they used to substitute its #to_s. Pass replacement.to_s, or a block, instead.
* **regexp:** String#scan, #sub and #gsub raise TypeError for a pattern that is neither a Regexp nor a String, where they used to match its #to_s literally. Pass the pattern as a String (pattern.to_s) or a Regexp instead.
* **guest:** the kobako-mruby harness names beni 0.21's types — the keyword Hash in `Arguments` and `PayloadCodec::encode_call_arguments` is `beni::RHash` — so a shell or codec built on these crates moves to beni 0.21 with them.

### Features

* **guest:** rebuild the guest crates on beni 0.21 ([8ec00ff](https://github.com/elct9620/kobako/commit/8ec00ff94ec21b6bbfe8b88e0a4e75f4489dd692))


### Bug Fixes

* **regexp:** compile a String pattern handed to String#match ([0669dcd](https://github.com/elct9620/kobako/commit/0669dcd675a6187a0e86122f93068707038b8031))
* **regexp:** convert a MatchData group that is neither number nor name ([f1b3be0](https://github.com/elct9620/kobako/commit/f1b3be088644c4100dc8b897c5b55be0030b2c79))
* **regexp:** define the match operator on Symbol and nil, not Kernel ([32d65c8](https://github.com/elct9620/kobako/commit/32d65c8c490b369294f28acd20702db2781fbfa4))
* **regexp:** describe an unrenderable replacement, and refuse a second escape text ([d95ae85](https://github.com/elct9620/kobako/commit/d95ae850c74b8025df6bad1477fa7ecd1ad4b199))
* **regexp:** hide the preserved String methods and the compile cache from guest code ([ceea471](https://github.com/elct9620/kobako/commit/ceea47172f2292e3a4b0ffae221b67fba797d19c))
* **regexp:** keep a pattern's source and options in Regexp.new ([c0d3874](https://github.com/elct9620/kobako/commit/c0d38745738e2c043ebfeb9759f3eea70df58575))
* **regexp:** read a non-Integer, non-String option by its truth ([a261e6f](https://github.com/elct9620/kobako/commit/a261e6f2cf3c8be457c50ad04b8c084c9ead806a))
* **regexp:** read a String as itself rather than through its #to_s ([ff99ff8](https://github.com/elct9620/kobako/commit/ff99ff8e4dc10b5774b4a98edf3075e231a36690))
* **regexp:** read a Symbol by its own name rather than its #to_s ([f29e37a](https://github.com/elct9620/kobako/commit/f29e37a749bb6653d5ddbe328a93c0b6c27492cc))
* **regexp:** refuse a pattern that is neither a Regexp nor a String ([0e6e061](https://github.com/elct9620/kobako/commit/0e6e061e670f82b167d4864a558569cf991c1655))
* **regexp:** refuse a replacement that is neither a String nor a Hash ([7a5327b](https://github.com/elct9620/kobako/commit/7a5327b49303bd93eaa29b2c85cf54d4425b82a6))
* **regexp:** refuse an unknown letter in a Regexp.new flag string ([e53f909](https://github.com/elct9620/kobako/commit/e53f9092340bc24facffab392d7e2a03855e6e0c))
* **regexp:** refuse to build a pattern from a value that is no String ([73aefe0](https://github.com/elct9620/kobako/commit/73aefe06c84af23022849262c4e48dac3d86e900))
* **regexp:** refuse to escape a value that is neither String nor Symbol ([55122bb](https://github.com/elct9620/kobako/commit/55122bb176abcf7a2e93faf9436e85f622fdac38))
* **regexp:** word a subject that is not text the way mruby does ([45f1f8c](https://github.com/elct9620/kobako/commit/45f1f8ca831012beb74a9ee73ccfb6bed288c7ca))


### Build System

* **guest:** rebuild the guest crates on beni 0.22 ([fd6aeb9](https://github.com/elct9620/kobako/commit/fd6aeb92705fa53c52e2cd75b4918a58baa6637e))

## [0.17.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.16.0...kobako-regexp-v0.17.0) (2026-09-20)


### ⚠ BREAKING CHANGES

* **guest:** a pattern's allocator is undefined from the moment the capability installs rather than from its first wrap, so allocating one is refused for the missing allocator throughout an invocation, where before the first statement could still reach an uninitialized carrier.
* **regexp:** `Kernel#=~` answers an ArgumentError for any operand count but one. `x =~ y` passes exactly one and is unaffected; reaching another count takes a `send`.
* **guest:** `Kobako::raise_transport_error`, `raise_service_error` and `reraise` are gone. A flow of its own builds `Kobako::transport_error` or `service_error` and hands the result back as `Err`, which beni raises at the guest call site.

### Features

* **guest:** rebuild the guest crates on beni 0.17 ([b397701](https://github.com/elct9620/kobako/commit/b397701231c7d8718503d24f73b175676d0d1be3))
* **guest:** rebuild the guest crates on beni 0.18 ([ae232eb](https://github.com/elct9620/kobako/commit/ae232ebbf708419ff0672172fbf8627363fb02da))


### Bug Fixes

* **regexp:** hold Kernel#=~ to the one operand it takes ([e94c075](https://github.com/elct9620/kobako/commit/e94c0759468e4a0911cd17d2eae74dd59dac1afc))

## [0.16.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.15.0...kobako-regexp-v0.16.0) (2026-09-15)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako crates versions

## [0.15.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.14.0...kobako-regexp-v0.15.0) (2026-09-13)


### Features

* **mruby:** upgrade beni to 0.14 and locate a parse failure ([586c910](https://github.com/elct9620/kobako/commit/586c910873710169c0dfce1fe7edb21f9e91cdea))
* **spec:** declare what the guest answers when a value will not cross ([886a16f](https://github.com/elct9620/kobako/commit/886a16f173efe44766dd9884ac8bfed61113728a))

## [0.14.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.13.1...kobako-regexp-v0.14.0) (2026-08-06)


### Bug Fixes

* **regexp:** stop raising where MRI answers no match ([d229974](https://github.com/elct9620/kobako/commit/d2299740a2a0206c88e7c397cdb72dc730c4c1cf))

## [0.13.1](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.13.0...kobako-regexp-v0.13.1) (2026-07-30)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako crates versions

## [0.13.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.12.0...kobako-regexp-v0.13.0) (2026-07-29)


### Bug Fixes

* **guest:** refuse text the capability gems cannot read as text ([539045a](https://github.com/elct9620/kobako/commit/539045a80192cc28c23a4bcbebc311c55eb138fb))

## [0.12.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.11.0...kobako-regexp-v0.12.0) (2026-07-24)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako crates versions

## [0.11.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.10.2...kobako-regexp-v0.11.0) (2026-07-19)


### Miscellaneous Chores

* release the guest crates at 0.11.0 ([83391c1](https://github.com/elct9620/kobako/commit/83391c15a2bd7b162495e851ad1603a047b0cf0e))

## [0.10.2](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.10.1...kobako-regexp-v0.10.2) (2026-07-18)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako crates versions

## [0.10.1](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.10.0...kobako-regexp-v0.10.1) (2026-07-17)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako crates versions

## [0.10.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.9.0...kobako-regexp-v0.10.0) (2026-07-12)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako crates versions

## [0.9.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.8.0...kobako-regexp-v0.9.0) (2026-07-11)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako crates versions

## [0.8.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.7.0...kobako-regexp-v0.8.0) (2026-07-08)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako crates versions

## [0.7.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.6.1...kobako-regexp-v0.7.0) (2026-07-03)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako crates versions

## [0.6.1](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.6.0...kobako-regexp-v0.6.1) (2026-07-02)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako crates versions

## [0.6.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.5.2...kobako-regexp-v0.6.0) (2026-06-26)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako guest crates versions

## [0.5.2](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.5.1...kobako-regexp-v0.5.2) (2026-06-24)


### Bug Fixes

* **dispatch:** keep short method names intact across kwarg unpacking ([c6e4a6f](https://github.com/elct9620/kobako/commit/c6e4a6f268970c0c2d2851d3a23e3bec153dc56d))

## [0.5.1](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.5.0...kobako-regexp-v0.5.1) (2026-06-14)


### Bug Fixes

* **guest:** adopt beni 0.7.0 protected dispatch (B-51) ([c61655b](https://github.com/elct9620/kobako/commit/c61655bcead336d32a4b6ff7ff1b34c21cdfccd9))

## [0.5.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.4.1...kobako-regexp-v0.5.0) (2026-06-12)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako guest crates versions

## [0.4.1](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.4.0...kobako-regexp-v0.4.1) (2026-06-11)


### Miscellaneous Chores

* **kobako-regexp:** Synchronize kobako guest crates versions

## [0.4.0](https://github.com/elct9620/kobako/compare/kobako-regexp-v0.3.0...kobako-regexp-v0.4.0) (2026-06-10)


### Features

* **regexp:** add Kernel#=~ fallback returning nil ([d461781](https://github.com/elct9620/kobako/commit/d4617815b8e888a09a548c7f4664819cdddc34c8))
* **regexp:** add regexp-aware String#[]= ([34807f5](https://github.com/elct9620/kobako/commit/34807f5ba5f6e17efff27af3c5c24ae42b0b651d))
* **regexp:** add Regexp.last_match and last_match= ([03649e8](https://github.com/elct9620/kobako/commit/03649e81cfda0c43ac22777f70ea38a0ac4a93c7))
* **regexp:** add Regexp#named_captures and #names ([7cf018d](https://github.com/elct9620/kobako/commit/7cf018d39529be1d1384297b1b38b9d1670523e7))
* **regexp:** add String#slice! ([2857e0e](https://github.com/elct9620/kobako/commit/2857e0ee55df9f6295a25391ff76613b8dd5d555))
* **regexp:** add the Ruby-to-fancy-regex pattern and flag translation layer ([3d5615d](https://github.com/elct9620/kobako/commit/3d5615d70b3a71a0cf0b5f0ce8acf756a13709c2))
* **regexp:** add the String regexp-integration methods ([de4bf7d](https://github.com/elct9620/kobako/commit/de4bf7d8097e6670b924fc2ddcdb45ecb68b58fb))
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
* **regexp:** implement the Regexp and MatchData classes over fancy-regex ([78623b8](https://github.com/elct9620/kobako/commit/78623b8147d0070769c274872f69fa26e923a161))
* **regexp:** memoize compiled patterns per invocation ([f764d66](https://github.com/elct9620/kobako/commit/f764d66a574da51b9e714db1ae6d917cce4cf611))
* **regexp:** raise IndexError for out-of-range MatchData#begin/#end/#offset ([85fc8d6](https://github.com/elct9620/kobako/commit/85fc8d67d0fcc137a7f43695ead34317d557ec1a))
* **regexp:** reproduce the C match-family operand handling ([9e30d2d](https://github.com/elct9620/kobako/commit/9e30d2d6d6ddd42c84d5e2fb55cc2c06076c4fd4))
* **regexp:** set the $+ last-group match global ([b65a424](https://github.com/elct9620/kobako/commit/b65a42434b2edd5b01ed879b09895d17ff778888))
* **regexp:** support length and Range forms of MatchData#[] ([7ac4dee](https://github.com/elct9620/kobako/commit/7ac4deec79a215b35fdc2786522145ce6d34263c))
* **regexp:** yield the MatchData to a block in Regexp#match / String#match ([f5a6e53](https://github.com/elct9620/kobako/commit/f5a6e53ec5993237805c2360fa70684f84acf6b8))


### Bug Fixes

* **regexp:** align String#=~ with MRI semantics ([c8f3e70](https://github.com/elct9620/kobako/commit/c8f3e70c9f0797c614aab1639934d280fe20b90a))
* **regexp:** bound backtracking, clamp match positions, harden engine errors ([0177f71](https://github.com/elct9620/kobako/commit/0177f71cccf61c140912aab3c2e639286fd768d0))
* **regexp:** correct String#split group and zero-width handling ([c0150bc](https://github.com/elct9620/kobako/commit/c0150bc04c5dccdd430b173219c42c8f607112d1))
* **regexp:** honour capturing groups and the limit arg in String#split ([66e7398](https://github.com/elct9620/kobako/commit/66e73984f8318b582cc7aa0db48deacfb10e8671))
* **regexp:** make Regexp.last_match= refresh the numbered globals ([e47b257](https://github.com/elct9620/kobako/commit/e47b257715b73263ae5d1f9bae67442197330ee0))
* **regexp:** name the pattern in match-time errors and snap String#index pos ([5835f42](https://github.com/elct9620/kobako/commit/5835f42ae6b27e2a2a0d0bc5bf8023024a20642a))
* **regexp:** stop escaping the slash in Regexp.escape ([6c6f17a](https://github.com/elct9620/kobako/commit/6c6f17a63dbddf30ccdba19ddf3c9b7fbb7772cd))


### Performance Improvements

* **regexp:** move the subject and spans into MatchData on a match ([870fdc4](https://github.com/elct9620/kobako/commit/870fdc49b90ab0af692a3cd80fb11b45e2e6d734))
