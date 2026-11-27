#!/usr/bin/env ruby
# frozen_string_literal: true

# Host-side async I/O demo: overlap many invocations' external I/O on a
# SINGLE OS thread using a Fiber scheduler (the `async` gem).
#
# The lesson this example encodes
# -------------------------------
# Each guest runs on a thread kobako keeps, so a Fiber whose run is inside
# a Service leaves its own Thread free for another Fiber's run. A Service
# that waits on scheduler-aware I/O therefore yields the Fiber, and the
# reactor starts or resumes another request while it waits:
#
#   guest  Request.(id)            -> plan: decide what to fetch
#   host   Resource::Fetcher.fetch -> the fiber yields here; another runs
#   guest  (same run)              -> render the final result
#
# The guest keeps every line of the request's logic; the host only owns the
# I/O. Many Fibers may run on one Sandbox at once, as on many Threads.
#
# The fetch latency is SIMULATED with `sleep` (intercepted by the fiber
# scheduler under Async) — the point is the concurrency structure, not a
# benchmark. Run with --sequential to see the same workload serialize when
# there is no reactor to overlap the waits.
#
# Usage:
#   ruby examples/async-io/app.rb                   # async reactor, one thread
#   ruby examples/async-io/app.rb --sequential      # blocking baseline
#   ruby examples/async-io/app.rb --count 8 --latency 250
#
# Parsing CLI flags before bundler/inline runs is deliberate: --sequential
# needs no reactor, so the `async` gem is only added to the inline Gemfile
# when the async mode will actually use it.

require "optparse"

options = { count: 5, latency: 300, sequential: false }
OptionParser.new do |opts|
  opts.banner = "Usage: ruby examples/async-io/app.rb [options]"
  opts.on("--count N", Integer, "Number of concurrent requests (default: 5)") do |count|
    options[:count] = count
  end
  opts.on("--latency MS", Integer, "Simulated fetch latency per request, ms (default: 300)") do |ms|
    options[:latency] = ms
  end
  opts.on("--sequential", "Blocking baseline: no reactor, fetches serialize") do
    options[:sequential] = true
  end
  opts.on("-h", "--help", "Show this help") do
    warn opts
    exit
  end
end.parse!

require "bundler/inline"

gemfile do
  source "https://rubygems.org"
  gem "kobako", "~> 0.32.0"
  gem "async", "~> 2.46" unless options[:sequential]
end

require "kobako"
require "async" unless options[:sequential]

# Example types are nested under AsyncIO so the file carries a single
# top-level constant and reads top-down.
module AsyncIO
  # The whole request, in the guest. kobako guests have no I/O capability by
  # construction — the build allowlist ships no I/O / network / sleep
  # mrbgem — so the guest reaches the outside world only through the Service
  # the host binds.
  REQUEST = <<~'MRUBY'
    Request = ->(id) {
      descriptor = "resource/#{id}"
      value = Resource::Fetcher.fetch(id, descriptor)
      "#{descriptor} => #{value}"
    }
  MRUBY

  # Monotonic event log. Each line carries the OS thread it ran on, so the
  # async run visibly stays on one thread while requests interleave.
  class Timeline
    def initialize
      @origin = monotonic
    end

    def record(task, event)
      elapsed_ms = ((monotonic - @origin) * 1000).round
      puts format("[+%<ms>5dms] [thread %<thread>d] [req %<req>d] %<event>s",
                  ms: elapsed_ms, thread: Thread.current.object_id, req: task, event: event)
    end

    private

    def monotonic
      Process.clock_gettime(Process::CLOCK_MONOTONIC)
    end
  end

  # The Service a run calls for its external fetch. `sleep` is intercepted
  # by the fiber scheduler under Async, so the waiting fiber yields while
  # its run is inside this call, and another request runs meanwhile. The
  # payload is a deterministic stand-in for whatever an HTTP body would
  # carry. One object serves every run, so it keeps no per-request state.
  class Fetcher
    def initialize(latency_ms, timeline)
      @latency = latency_ms / 1000.0
      @timeline = timeline
    end

    def fetch(id, descriptor)
      @timeline.record(id, "fetch:start  #{descriptor}")
      sleep(@latency)
      @timeline.record(id, "fetch:done")
      descriptor.split("/").last.reverse
    end
  end

  # One Sandbox for every request: each run gets its own guest, so the
  # concurrent Fibers share no guest state.
  def self.build_sandbox(fetcher)
    sandbox = Kobako::Sandbox.new
    sandbox.bind("Resource::Fetcher", fetcher)
    sandbox.preload(code: REQUEST, name: :Request)
    sandbox
  end

  # One request: a single run that plans, fetches through the Service, and
  # renders.
  def self.process(sandbox, timeline, id)
    timeline.record(id, "run:start")
    result = sandbox.run(:Request, id).value
    timeline.record(id, "run:done     -> #{result}")
    result
  end

  # Runs every request concurrently on a single-thread async reactor. The
  # fetches overlap because the reactor schedules a waiting fiber's peers
  # while it sleeps inside the Service.
  def self.run_async(count, sandbox, timeline)
    measure do
      Async do
        # Spawn one child task per request, then wait for every result. The
        # reactor runs the children concurrently on this single thread.
        tasks = Array.new(count) { |id| Async { process(sandbox, timeline, id) } }
        tasks.map(&:wait)
      end.wait
    end
  end

  # Blocking baseline: no reactor, so each request's fetch fully blocks the
  # thread and the total is the sum of the latencies.
  def self.run_sequential(count, sandbox, timeline)
    measure { Array.new(count) { |id| process(sandbox, timeline, id) } }
  end

  # Runs the block and returns [block_result, elapsed_seconds].
  def self.measure
    start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    value = yield
    [value, Process.clock_gettime(Process::CLOCK_MONOTONIC) - start]
  end
end

timeline = AsyncIO::Timeline.new
sandbox = AsyncIO.build_sandbox(AsyncIO::Fetcher.new(options[:latency], timeline))
mode = options[:sequential] ? "sequential (blocking)" : "async reactor (single thread)"

puts "async-io demo: #{options[:count]} requests, ~#{options[:latency]}ms simulated fetch each"
puts "mode: #{mode}"
puts

results, elapsed =
  if options[:sequential]
    AsyncIO.run_sequential(options[:count], sandbox, timeline)
  else
    AsyncIO.run_async(options[:count], sandbox, timeline)
  end

serialized_ms = options[:count] * options[:latency]
puts
puts format("total wall time:        %5dms", (elapsed * 1000).round)
puts format("serialized would spend: %5dms (count x latency)", serialized_ms)
puts "results: #{results.inspect}"
