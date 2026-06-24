#!/usr/bin/env ruby
# frozen_string_literal: true

# Minimal serverless demo: a Rack app that dispatches GET /:name to an
# operator-provided mruby script. The script body is preloaded as a named
# entrypoint constant and invoked via #run(:Entrypoint, Rack::Request.new(env)),
# returning a Rack response triplet [status, headers, body] directly.
#
# Two sandbox strategies, selected by --pool:
#
#   * default (per-request): each request builds a fresh Kobako::Sandbox,
#     preloads only the matched route, and invokes it once. The strongest
#     isolation kobako offers; Sandbox.new + #preload sit on the hot path.
#   * --pool: a Kobako::Pool of long-lived Sandboxes, each preloaded with
#     every route on first checkout; each request checks one out and
#     dispatches #run on it. Per-invocation guest isolation is unchanged —
#     every #run still runs a fresh mrb_state (SPEC B-03) — while
#     Sandbox.new + #preload move off the hot path.
#
# The Rack::Request is a non-wire-representable host object, so kobako's
# #run host→guest auto-wrap (SPEC B-34) allocates a Handle for
# it; the guest receives a Kobako::Handle proxy whose method calls
# (request_method, path, params) round-trip back through RPC. No
# host-side marshalling step sits between the Rack env and the script.
#
# Usage:
#   ruby examples/serverless/app.rb                       # Puma, per-request
#   ruby examples/serverless/app.rb --type falcon
#   ruby examples/serverless/app.rb --pool                # reuse a pool of Sandboxes
#   ruby examples/serverless/app.rb --pool --pool-size 8
#
# Parsing CLI flags before bundler/inline runs is deliberate: only the
# server gem the operator picked is added to the inline Gemfile, so
# running with --type puma does not pull Falcon's dependency tree.

require "optparse"

SERVER_TYPES = %w[puma falcon].freeze
DEFAULT_POOL_SIZE = 5

options = { type: "puma", pool: false, pool_size: DEFAULT_POOL_SIZE }
OptionParser.new do |opts|
  opts.banner = "Usage: ruby examples/serverless/app.rb [options]"
  opts.on("--type TYPE", SERVER_TYPES,
          "Rack handler to start (#{SERVER_TYPES.join(", ")}; default: puma)") do |type|
    options[:type] = type
  end
  opts.on("--pool", "Reuse a pool of preloaded Sandboxes instead of building one per request") do
    options[:pool] = true
  end
  opts.on("--pool-size N", Integer, "Number of pooled Sandboxes (default: #{DEFAULT_POOL_SIZE})") do |size|
    options[:pool_size] = size
  end
  opts.on("-h", "--help", "Show this help") do
    warn opts
    exit
  end
end.parse!

require "bundler/inline"

gemfile do
  source "https://rubygems.org"
  gem "kobako", "~> 0.11.0"
  gem "rack", "~> 3.0"
  gem "rackup", "~> 2.0"
  gem options[:type]
end

require "kobako"
require "rack"
require "rackup"

# Example types are nested under Serverless so the script has a single
# top-level constant — Rubocop's Style/OneClassPerFile is happy and the
# example reads top-down without splitting into multiple files.
module Serverless
  # Operator-managed script table — keys are the route segment after `/`,
  # values are a +[Entrypoint, source]+ pair: +Entrypoint+ is the
  # top-level constant the +source+ defines, used both as the +#preload+
  # name and the +#run+ target. Each entrypoint is a callable accepting
  # one argument — a +Kobako::Handle+ proxy of a host-side +Rack::Request+
  # whose +req.params+ / +req.request_method+ / +req.path+ calls dispatch
  # back to the host as one RPC round-trip.
  ROUTES = {
    "hello" => [:Hello, <<~'MRUBY'],
      Hello = ->(req) {
        name = (req.params["name"] || "world")
        [200, { "content-type" => "text/plain" }, ["Hello, #{name}!\n"]]
      }
    MRUBY
    "echo" => [:Echo, <<~'MRUBY'],
      Echo = ->(req) {
        body = "method: #{req.request_method}\npath: #{req.path}\nquery: #{req.params.inspect}\n"
        [200, { "content-type" => "text/plain" }, [body]]
      }
    MRUBY
    "sum" => [:Sum, <<~'MRUBY'],
      Sum = ->(req) {
        a = req.params["a"].to_i
        b = req.params["b"].to_i
        [200, { "content-type" => "text/plain" }, ["#{a} + #{b} = #{a + b}\n"]]
      }
    MRUBY
    "shout" => [:Shout, <<~'MRUBY']
      Shout = ->(req) {
        msg = req.params["msg"] || ""
        [200, { "content-type" => "text/plain" }, ["#{msg.upcase}!\n"]]
      }
    MRUBY
  }.freeze

  # Builds a fresh +Kobako::Sandbox+ per request, preloads only the matched
  # route's entrypoint, and invokes it once — the original per-request
  # demo behaviour, selected when +--pool+ is off. Concurrent requests
  # cannot share guest state because each gets its own +mrb_state+.
  class PerRequestInvoker
    def invoke(entry, rack_env)
      entrypoint, source = entry
      sandbox = Kobako::Sandbox.new
      sandbox.preload(code: source, name: entrypoint)
      sandbox.run(entrypoint, Rack::Request.new(rack_env))
    end
  end

  # Hands each request a warm +Kobako::Sandbox+ from a +Kobako::Pool+,
  # each preloaded with every route's entrypoint, and dispatches +#run+ on
  # the checked-out Sandbox. Reuse keeps the same per-invocation guest
  # isolation (each +#run+ runs a fresh +mrb_state+, SPEC B-03) while
  # moving +Sandbox.new+ + +#preload+ off the hot path. The Pool block is
  # the per-Sandbox setup window — it runs once per constructed Sandbox.
  # Exclusive checkout is the Pool's contract, not an optimisation: the
  # host-side Sandbox carries per-invocation state, so two threads sharing
  # one would interleave captures and Handles across requests.
  class PooledInvoker
    def initialize(routes, size:)
      @pool = Kobako::Pool.new(slots: size) do |sandbox|
        routes.each_value { |entrypoint, source| sandbox.preload(code: source, name: entrypoint) }
      end
    end

    def invoke(entry, rack_env)
      entrypoint, = entry
      @pool.with { |sandbox| sandbox.run(entrypoint, Rack::Request.new(rack_env)) }
    end
  end

  # Rack-compatible application. Routes +GET /:name+ to +ROUTES[name]+ and
  # delegates the matched entrypoint's execution to an injected invoker
  # (+PerRequestInvoker+ or +PooledInvoker+), so the routing and error
  # mapping here are identical regardless of the sandbox strategy. The
  # root path +/+ lists the available scripts so the demo is
  # self-discoverable.
  class App
    PLAIN = { "content-type" => "text/plain" }.freeze

    def initialize(routes, invoker)
      @routes = routes
      @invoker = invoker
    end

    def call(env)
      return method_not_allowed unless env["REQUEST_METHOD"] == "GET"

      name = env["PATH_INFO"].sub(%r{\A/}, "")
      return index if name.empty?

      entry = @routes[name]
      return not_found(name) unless entry

      @invoker.invoke(entry, env)
    rescue Kobako::PoolTimeoutError => e
      pool_exhausted(e)
    rescue Kobako::TrapError, Kobako::SandboxError, Kobako::ServiceError => e
      sandbox_error(e)
    end

    private

    def index
      list = @routes.keys.map { |n| "  GET /#{n}" }.join("\n")
      [200, PLAIN, ["Available scripts:\n#{list}\n"]]
    end

    def not_found(name)
      [404, PLAIN, ["no script registered for /#{name}\n"]]
    end

    def method_not_allowed
      [405, { "content-type" => "text/plain", "allow" => "GET" }, ["only GET is supported\n"]]
    end

    def sandbox_error(error)
      [500, PLAIN, ["#{error.class}: #{error.message}\n"]]
    end

    def pool_exhausted(error)
      [503, { "content-type" => "text/plain", "retry-after" => "1" },
       ["sandbox pool exhausted: #{error.message}\n"]]
    end
  end
end

if __FILE__ == $PROGRAM_NAME
  port = Integer(ENV.fetch("PORT", "9292"))
  host = ENV.fetch("HOST", "127.0.0.1")

  invoker =
    if options[:pool]
      Serverless::PooledInvoker.new(Serverless::ROUTES, size: options[:pool_size])
    else
      Serverless::PerRequestInvoker.new
    end
  app = Serverless::App.new(Serverless::ROUTES, invoker)

  handler = Rackup::Handler.get(options[:type])
  mode = options[:pool] ? "pool=#{options[:pool_size]}" : "per-request"
  warn "Serverless demo on http://#{host}:#{port} (handler: #{handler.name}, sandbox: #{mode})"
  handler.run(app, Host: host, Port: port)
end
