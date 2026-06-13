#!/usr/bin/env ruby
# frozen_string_literal: true

# Host-side async I/O demo: overlap many Sandboxes' external I/O on a
# SINGLE OS thread using a Fiber scheduler (the `async` gem).
#
# The lesson this example encodes
# -------------------------------
# kobako's wasm dispatch is synchronous and GVL-held, and Runtime holds at
# most one active Invocation per OS thread; SPEC lists async / yield-resume
# execution as out of scope. So you CANNOT do blocking I/O inside a Service
# dispatch and then Fiber.yield around it — the wasm frame is still on the
# native stack, and suspending it would corrupt the per-thread invocation
# state.
#
# The safe shape is host-orchestrated continuation: the guest is a PURE
# function, and all external I/O happens in host Ruby BETWEEN invocations,
# where no wasm frame sits on the stack. At that point a Fiber scheduler is
# free to suspend the waiting fiber and run another request's compute,
# overlapping I/O waits across Sandboxes without spending a thread per
# in-flight request.
#
#   guest #run(:Plan, id)      -> pure compute: decide what to fetch
#   --- invocation unwound; no wasm frame on the native stack ---
#   host fetch (async I/O)      -> the fiber yields here; another task runs
#   guest #run(:Render, data)  -> pure compute: build the final result
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
  gem "kobako", "~> 0.10.0"
  gem "async", "~> 2.0" unless options[:sequential]
end

require "kobako"
require "async" unless options[:sequential]

# Example types are nested under AsyncIO so the file carries a single
# top-level constant and reads top-down.
module AsyncIO
  # Phase 1 entrypoint. kobako guests have no I/O capability by
  # construction — the build allowlist ships no I/O / network / sleep
  # mrbgem — so the guest only decides WHICH external resource it needs and
  # returns a descriptor. The host performs the fetch outside any dispatch
  # frame.
  PLAN = <<~'MRUBY'
    Plan = ->(id) { "resource/#{id}" }
  MRUBY

  # Phase 2 entrypoint. Takes the host-fetched payload — a wire Hash with
  # string keys (positional Hash args round-trip their key types verbatim)
  # — and produces the final result. Still pure compute.
  RENDER = <<~'MRUBY'
    Render = ->(data) { "#{data['descriptor']} => #{data['value']}" }
  MRUBY

  # Builds a Sandbox with both phases preloaded. Each concurrent request
  # owns one Sandbox: a Sandbox carries host-side per-invocation state, so
  # two fibers must never share one (the same exclusive-use rule the
  # serverless example's pool enforces).
  def self.build_sandbox
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: PLAN, name: :Plan)
    sandbox.preload(code: RENDER, name: :Render)
    sandbox
  end

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

  # Simulated external fetch. `sleep` is intercepted by the fiber scheduler
  # under Async, so the waiting fiber yields and another request's compute
  # runs while this one waits. The payload is a deterministic stand-in for
  # whatever an HTTP body would carry.
  class Fetcher
    def initialize(latency_ms)
      @latency = latency_ms / 1000.0
    end

    def fetch(descriptor)
      sleep(@latency)
      descriptor.split("/").last.reverse
    end
  end

  # One request's two-phase flow: plan (compute) -> fetch (I/O) -> render
  # (compute). The fetch sits between two #run calls, never inside one.
  class Request
    def initialize(sandbox, fetcher, timeline)
      @sandbox = sandbox
      @fetcher = fetcher
      @timeline = timeline
    end

    def process(id)
      @timeline.record(id, "plan:start")
      descriptor = @sandbox.run(:Plan, id)
      @timeline.record(id, "fetch:start  #{descriptor}")
      value = @fetcher.fetch(descriptor)
      @timeline.record(id, "fetch:done")
      result = @sandbox.run(:Render, { "descriptor" => descriptor, "value" => value })
      @timeline.record(id, "render:done  -> #{result}")
      result
    end
  end

  # One Request per concurrent task, each owning its own Sandbox so two
  # fibers never share host-side per-invocation state.
  def self.build_requests(count, fetcher, timeline)
    Array.new(count) { Request.new(build_sandbox, fetcher, timeline) }
  end

  # Runs every request concurrently on a single-thread async reactor. The
  # fetches overlap because the reactor schedules a waiting fiber's peers
  # while it sleeps.
  def self.run_async(count, fetcher, timeline)
    requests = build_requests(count, fetcher, timeline)
    measure { await_all(requests) }
  end

  def self.await_all(requests)
    Async do
      # Spawn one child task per request, then wait for every result. The
      # reactor runs the children concurrently on this single thread.
      tasks = requests.each_with_index.map { |req, id| Async { req.process(id) } }
      tasks.map(&:wait)
    end.wait
  end

  # Blocking baseline: no reactor, so each request's fetch fully blocks the
  # thread and the total is the sum of the latencies.
  def self.run_sequential(count, fetcher, timeline)
    requests = build_requests(count, fetcher, timeline)
    measure { requests.each_with_index.map { |req, id| req.process(id) } }
  end

  # Runs the block and returns [block_result, elapsed_seconds].
  def self.measure
    start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    value = yield
    [value, Process.clock_gettime(Process::CLOCK_MONOTONIC) - start]
  end
end

fetcher = AsyncIO::Fetcher.new(options[:latency])
timeline = AsyncIO::Timeline.new
mode = options[:sequential] ? "sequential (blocking)" : "async reactor (single thread)"

puts "async-io demo: #{options[:count]} requests, ~#{options[:latency]}ms simulated fetch each"
puts "mode: #{mode}"
puts

results, elapsed =
  if options[:sequential]
    AsyncIO.run_sequential(options[:count], fetcher, timeline)
  else
    AsyncIO.run_async(options[:count], fetcher, timeline)
  end

serialized_ms = options[:count] * options[:latency]
puts
puts format("total wall time:        %5dms", (elapsed * 1000).round)
puts format("serialized would spend: %5dms (count x latency)", serialized_ms)
puts "results: #{results.inspect}"
