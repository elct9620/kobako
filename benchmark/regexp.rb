# frozen_string_literal: true

# Characterization benchmark (not in release gate) — the standalone regexp
# capability profile. Two groups:
#
#   Since Regexp lives in the opt-in variant binaries, the gated suite no
#   longer covers it; this characterization is the sole regexp perf guard.
#
#   Compile (10a–10e) decomposes the literal-in-a-hot-loop cost. mruby
#   recompiles a `/.../` literal on every evaluation, so 10a re-evaluates the
#   literal each iteration while 10b/10c hoist it out; 10d isolates compilation
#   with no matching. 10a − 10b is the recompilation cost the per-invocation
#   compile cache (docs/regexp.md RX-08) removes, so this group is the guard
#   that the cache keeps paying off.
#
#   Operations (10f–10i) hoist the pattern so they measure fancy-regex matching
#   throughput rather than compilation: a capturing match, scan over repeated
#   matches, a gsub with a block, and a split on a delimiter pattern.
#
#   10a — literal-in-loop `=~`: the literal recompiles every iteration.
#   10b — hoisted `=~`: the same match, compiled once. 10a − 10b is compile cost.
#   10c — hoisted `match?`: drops the MatchData build and global refresh `=~` does.
#   10d — compile-only: `Regexp.compile` 1000 times with no match.
#   10e — empty loop: the 1000-iteration loop overhead alone.
#   10f — capturing `match` against a short subject.
#   10g — `scan` collecting every word of a sentence.
#   10h — `gsub` upcasing every word with a block.
#   10i — `split` on a comma-and-space delimiter pattern.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "guest"
require "paths"
require "runner"

runner = Kobako::Bench::Runner.new("regexp")

# Regexp lives only in the variant binaries — drive the unicode variant,
# the fullest surface. memory_limit: nil keeps the per-invocation delta cap
# out of the hot loop so the cases measure VM work, matching mruby_eval's #4.
REGEXP_WASM = Kobako::Bench::Paths.variant_wasm("regexp-unicode")
sandbox = Kobako::Sandbox.new(wasm_path: Kobako::Bench::Guest.path(REGEXP_WASM), memory_limit: nil)
sandbox.eval("nil") # warm

SUBJECT = '"the quick brown bar jumps"'

COMPILE_LITERAL_LOOP = <<~RUBY.freeze
  h = 0
  1000.times { h += 1 if /foo|bar|baz/ =~ #{SUBJECT} }
  h
RUBY

COMPILE_HOISTED = <<~RUBY.freeze
  re = /foo|bar|baz/
  h = 0
  1000.times { h += 1 if re =~ #{SUBJECT} }
  h
RUBY

COMPILE_MATCH_P = <<~RUBY.freeze
  re = /foo|bar|baz/
  h = 0
  1000.times { h += 1 if re.match?(#{SUBJECT}) }
  h
RUBY

COMPILE_ONLY = <<~RUBY
  1000.times { Regexp.compile("foo|bar|baz") }
  0
RUBY

EMPTY_LOOP = <<~RUBY
  h = 0
  1000.times { h += 1 }
  h
RUBY

MATCH = <<~RUBY
  re = /(\\w+)@(\\w+)/
  s = "contact alice@example then bob@sample today"
  n = 0
  1000.times { n += 1 if re.match(s) }
  n
RUBY

SCAN = <<~RUBY
  re = /\\w+/
  s = "the quick brown fox jumps over the lazy dog"
  n = 0
  1000.times { n = s.scan(re).length }
  n
RUBY

GSUB = <<~RUBY
  re = /\\w+/
  s = "the quick brown fox jumps over the lazy dog"
  out = ""
  1000.times { out = s.gsub(re) { |w| w.upcase } }
  out.length
RUBY

SPLIT = <<~RUBY
  re = /,\\s*/
  s = "a, b,c,  d,e,f"
  n = 0
  1000.times { n = s.split(re).length }
  n
RUBY

runner.case_with_usage("10a-compile-literal-loop", sandbox) { sandbox.eval(COMPILE_LITERAL_LOOP) }
runner.case_with_usage("10b-compile-hoisted", sandbox) { sandbox.eval(COMPILE_HOISTED) }
runner.case_with_usage("10c-compile-match-p", sandbox) { sandbox.eval(COMPILE_MATCH_P) }
runner.case_with_usage("10d-compile-only", sandbox) { sandbox.eval(COMPILE_ONLY) }
runner.case_with_usage("10e-empty-loop", sandbox) { sandbox.eval(EMPTY_LOOP) }
runner.case_with_usage("10f-match", sandbox) { sandbox.eval(MATCH) }
runner.case_with_usage("10g-scan", sandbox) { sandbox.eval(SCAN) }
runner.case_with_usage("10h-gsub", sandbox) { sandbox.eval(GSUB) }
runner.case_with_usage("10i-split", sandbox) { sandbox.eval(SPLIT) }

puts runner.write!
