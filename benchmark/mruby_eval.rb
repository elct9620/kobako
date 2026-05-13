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

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

runner = Kobako::Bench::Runner.new("mruby_eval")

sandbox = Kobako::Sandbox.new
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

runner.case("4a-arith-100k-sum") { sandbox.run(ARITH_SCRIPT) }
runner.case("4b-string-concat-1000") { sandbox.run(STRING_SCRIPT) }
runner.case("4c-exception-raise-rescue-100") { sandbox.run(EXCEPTION_SCRIPT) }

puts runner.write!
