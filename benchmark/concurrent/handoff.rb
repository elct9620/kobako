# frozen_string_literal: true

# Characterization benchmark (not in release gate) — the waits between a
# run's caller and its guest's thread. Each wait spins before it sleeps,
# and at most half the CPUs spin at once, so the cost of a run moves with
# how many Threads wait at the same time and how long they wait:
#
#   threads — TH Threads share one Sandbox, each running a script that
#             makes CALLS Service calls, for a fixed window. Records
#             runs/s and the process CPU each run cost; a spin shows as
#             CPU, a sleep as a slower run.
#   gap     — one Thread waits GAP before each run, so the guest's thread
#             is idle when the run hands it over. Records the median run.
#   mixed   — two Threads run a long compute while three run 1-call
#             scripts, so long waits hold spin slots short ones need.
#
# Wall-clock, as in gvl_scheduling.rb: a wait that sleeps instead of
# spinning costs time a CPU-time sum cannot see.

$LOAD_PATH.unshift File.expand_path("../../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("../support", __dir__)

require "kobako"
require "guest"
require "runner"

GUEST = Kobako::Bench::Guest.path
WINDOW = 1.0 # seconds each throughput cell runs
THREAD_COUNTS = [1, 2, 5, 16, 32].freeze
CALL_COUNTS = [0, 1, 10].freeze
GAPS_US = [0, 2000].freeze
GAP_SAMPLES = 400
COMPUTE = "acc = 0; 60_000.times { |i| acc = (acc + i * 7) % 1_000_003 }; acc"

def now = Process.clock_gettime(Process::CLOCK_MONOTONIC)
def cpu = Process.times.then { |t| t.utime + t.stime }

# A warmed Sandbox in +mode+ whose +Entry+ makes +calls+ Service calls, or
# runs the compute loop when +calls+ is nil.
def sandbox(mode, calls)
  body = calls ? "#{calls}.times { Bench::Svc.call }; 1" : COMPUTE
  Kobako::Sandbox.new(wasm_path: GUEST, gvl: mode).tap do |s|
    s.bind("Bench::Svc", -> { 1 })
    s.preload(code: "Entry = ->(_) { #{body} }", name: :Entry)
    5.times { s.run(:Entry, 0) }
  end
end

# Runs each of +count+ Threads finishes on +sandbox+ before the window ends.
def runs_within_window(sandbox, count, stop)
  Array.new(count) do
    Thread.new do
      runs = 0
      while now < stop
        sandbox.run(:Entry, 0)
        runs += 1
      end
      runs
    end
  end
end

# Append +row+ to the suite and print +line+ for the reader at the terminal.
def record(runner, line, **row)
  runner.results << row
  puts line
end

def threads_cell(runner, mode, count, calls)
  shared = sandbox(mode, calls)
  started = cpu
  runs = runs_within_window(shared, count, now + WINDOW).sum(&:value)
  rate = runs / WINDOW
  cpu_us = (cpu - started) / runs * 1e6
  record(runner, format("threads %<m>-7s n=%<n>-2d calls=%<c>-2d %<r>9.0f runs/s  cpu %<u>7.1f us/run",
                        m: mode, n: count, c: calls, r: rate, u: cpu_us),
         label: "threads-#{mode}-#{count}-calls-#{calls}", ops_per_sec: rate, cpu_per_run_us: cpu_us)
end

# One run's wall time after the calling Thread stayed away for +gap_us+.
def run_after_gap(sandbox, gap_us)
  idle_until = now + (gap_us / 1e6)
  Thread.pass while now < idle_until
  started = now
  sandbox.run(:Entry, 0)
  now - started
end

def gap_cell(runner, mode, gap_us)
  warm = sandbox(mode, 1)
  p50 = Array.new(GAP_SAMPLES) { run_after_gap(warm, gap_us) }.sort[GAP_SAMPLES / 2]
  record(runner, format("gap     %<m>-7s gap=%<g>-5dus p50 %<p>7.1f us", m: mode, g: gap_us, p: p50 * 1e6),
         label: "gap-#{mode}-#{gap_us}us", wall_time: p50)
end

def mixed_cell(runner)
  long = sandbox(:release, nil)
  short = sandbox(:release, 1)
  stop = now + WINDOW
  longs = runs_within_window(long, 2, stop)
  shorts = runs_within_window(short, 3, stop)
  rates = { short: shorts.sum(&:value) / WINDOW, long: longs.sum(&:value) / WINDOW }
  rates.each do |kind, rate|
    record(runner, format("mixed   release %<k>-5s %<r>9.0f runs/s", k: kind, r: rate),
           label: "mixed-release-#{kind}", ops_per_sec: rate)
  end
end

runner = Kobako::Bench::Runner.new("handoff")
%i[release hold].each do |mode|
  THREAD_COUNTS.each { |count| CALL_COUNTS.each { |calls| threads_cell(runner, mode, count, calls) } }
  GAPS_US.each { |gap_us| gap_cell(runner, mode, gap_us) }
end
mixed_cell(runner)

puts runner.write
