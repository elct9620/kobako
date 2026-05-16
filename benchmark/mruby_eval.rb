# frozen_string_literal: true

# SPEC.md "Regression benchmarks" #4 — mruby script evaluation
# time. SPEC: "Impact of build_config/wasi.rb flag changes on VM
# execution speed." No RPC: every case is a self-contained mruby
# computation whose only host cost is the constant Sandbox#run
# overhead.
#
#   4a — integer arithmetic loop (sum of first 100k integers)
#   4b — string concatenation (1000 appends to a String)
#   4c — exception raise/rescue 100 times (exercises the
#        setjmp/longjmp path enforced by SPEC's invariant on
#        mruby exception unwind)
#   4d — Regexp match in a hot loop (mruby-onig-regexp / Onigmo
#        engine added to build_config/wasi.rb; verifies the guest
#        Regexp execution path that has no other regression guard)
#   4e — stdout puts loop, well below stdout_limit (exercises the
#        full B-04 IO path: mrblib IO#write → kobako_io_fwrite C
#        bridge → WASI pipe → host capture buffer; baseline cost
#        per buffered write)
#   4f — stdout cap saturation (2048 puts of 1023 bytes ≈ 2 MiB
#        against the 1 MiB default stdout_limit; the first ~1024
#        writes land in the WASI pipe and the rest are silently
#        dropped by the cap. Measures the cap-rejection path —
#        guest puts does not raise, the pipe returns short, and
#        sandbox.stdout_truncated? flips to true.)

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

runner = Kobako::Bench::Runner.new("mruby_eval")

# memory_limit: nil disables the default 5 MiB per-run guest memory
# cap so long benchmark-ips loops do not accumulate mruby heap into
# a trap (4c's repeated raise/rescue is the canonical trigger). The
# cap itself is exercised separately at the cold_start / sandbox
# construction level — #4 is about VM throughput, not cap behavior.
# stdout_limit stays at the default 1 MiB so 4f saturates the cap
# without an explicit override.
sandbox = Kobako::Sandbox.new(memory_limit: nil)
sandbox.run("nil") # warm

# build_config/wasi.rb pins guest mruby to MRB_INT32; a 100k sum
# overflows i32, so the loop body avoids accumulating into the
# counter and the script returns the iteration count only.
ARITH_SCRIPT = <<~RUBY
  acc = 0
  100_000.times { |i| acc ^= i }
  acc
RUBY

STRING_SCRIPT = <<~RUBY
  s = +""
  1000.times { s << "x" }
  s.length
RUBY

EXCEPTION_SCRIPT = <<~RUBY
  count = 0
  100.times do
    begin
      raise "boom"
    rescue
      count += 1
    end
  end
  count
RUBY

# 1000 Onigmo matches against a short subject. Pattern is forced to
# re-evaluate alternation on every iteration so the loop measures
# Regexp#=~ throughput, not literal-string fast paths.
REGEXP_SCRIPT = <<~RUBY
  hits = 0
  1000.times do
    hits += 1 if /foo|bar|baz/ =~ "the quick brown bar jumps"
  end
  hits
RUBY

# 1000 puts of 64 bytes each ≈ 65 KiB — comfortably below the 1 MiB
# stdout_limit so every write succeeds. Exercises the full IO path
# without crossing the cap.
STDOUT_LOOP_SCRIPT = <<~RUBY
  1000.times { puts "x" * 64 }
RUBY

# Attempt 2 MiB of stdout writes against the 1 MiB default cap.
# Guest puts does not raise on cap rejection — the WASI pipe
# returns short and the loop continues. sandbox.stdout_truncated?
# is true after the run.
STDOUT_NEAR_CAP_SCRIPT = <<~RUBY
  2048.times { puts "x" * 1023 }
RUBY

runner.case("4a-arith-100k-sum") { sandbox.run(ARITH_SCRIPT) }
runner.case("4b-string-concat-1000") { sandbox.run(STRING_SCRIPT) }
runner.case("4c-exception-raise-rescue-100") { sandbox.run(EXCEPTION_SCRIPT) }
runner.case("4d-regexp-match-1000") { sandbox.run(REGEXP_SCRIPT) }
runner.case("4e-stdout-puts-1000") { sandbox.run(STDOUT_LOOP_SCRIPT) }
runner.case("4f-stdout-cap-saturation") { sandbox.run(STDOUT_NEAR_CAP_SCRIPT) }

puts runner.write!
