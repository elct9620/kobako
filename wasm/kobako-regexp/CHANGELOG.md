# Changelog

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
