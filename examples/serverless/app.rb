#!/usr/bin/env ruby
# frozen_string_literal: true

# Minimal serverless demo: a Rack app that dispatches GET /:name to an
# operator-provided mruby script. The script body is preloaded as a named
# entrypoint constant and invoked via #run(:Entrypoint, GuestRequest.new(env)),
# returning a Rack response triplet [status, headers, body] directly.
#
# A Sandbox holds no state from any run: every #run gets a fresh mrb_state
# and its own Handles, so the invocation is the unit of isolation and the
# Sandbox is the unit of reuse. The app keeps one Sandbox per route, with
# only that route preloaded, in a cache of bounded size. A route's first
# request pays the Sandbox's setup and the one compile of its snippet;
# every later request only replays it.
#
# The request is a non-wire-representable host object, so kobako's
# #run host→guest auto-wrap allocates a Handle for
# it; the guest receives a Kobako::Handle proxy whose method calls
# round-trip back through RPC. A Handle exposes only what its object
# declares, so GuestRequest names the Rack::Request methods a script may
# call. No host-side marshalling step sits between the Rack env and the
# script.
#
# Usage:
#   ruby examples/serverless/app.rb                # Puma
#   ruby examples/serverless/app.rb --type falcon
#   ruby examples/serverless/app.rb --gvl release  # guests run in parallel across Threads
#   ruby examples/serverless/app.rb --cache-size 2 # keep at most 2 Sandboxes
#
# Parsing CLI flags before bundler/inline runs is deliberate: only the
# server gem the operator picked is added to the inline Gemfile, so
# running with --type puma does not pull Falcon's dependency tree.

require "optparse"

SERVER_TYPES = %w[puma falcon].freeze
GVL_MODES = %w[hold release].freeze
DEFAULT_CACHE_SIZE = 5

options = { type: "puma", gvl: "hold", cache_size: DEFAULT_CACHE_SIZE }
OptionParser.new do |opts|
  opts.banner = "Usage: ruby examples/serverless/app.rb [options]"
  opts.on("--type TYPE", SERVER_TYPES,
          "Rack handler to start (#{SERVER_TYPES.join(", ")}; default: puma)") do |type|
    options[:type] = type
  end
  opts.on("--gvl MODE", GVL_MODES, "GVL mode of each Sandbox (#{GVL_MODES.join(", ")}; default: hold)") do |mode|
    options[:gvl] = mode
  end
  opts.on("--cache-size N", Integer, "Most Sandboxes kept at once (default: #{DEFAULT_CACHE_SIZE})") do |size|
    options[:cache_size] = size
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
  gem "rack", "~> 3.2"
  gem "rackup", "~> 2.3"
  gem options[:type]
end

require "kobako"
require "rack"
require "rackup"

# Example types are nested under Serverless so the script has a single
# top-level constant — Rubocop's Style/OneClassPerFile is happy and the
# example reads top-down without splitting into multiple files.
module Serverless
  # The request a script receives. Most of Rack::Request's methods come from
  # Rack's helper modules, which a guest cannot reach by default, so it
  # declares the ones a script may call.
  class GuestRequest < Rack::Request
    GUEST_METHODS = %i[request_method path params get_header].freeze

    private

    def respond_to_guest?(name) = GUEST_METHODS.include?(name)
  end

  # Operator-managed script table — keys are the route segment after `/`,
  # values are a +[Entrypoint, source]+ pair: +Entrypoint+ is the
  # top-level constant the +source+ defines, used both as the +#preload+
  # name and the +#run+ target. Each entrypoint is a callable accepting
  # one argument — a +Kobako::Handle+ proxy of a host-side +GuestRequest+
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

  # At most +size+ Sandboxes, one per route, each with only its route's
  # entrypoint preloaded. A Sandbox may serve many Threads at once, so a
  # request takes it without checking it out; the lock guards only the
  # table. Past +size+, the route used longest ago gives its Sandbox up.
  class SandboxCache
    def initialize(size:, gvl:)
      @size = size
      @gvl = gvl
      @sandboxes = {}
      @lock = Mutex.new
    end

    def fetch(entrypoint, source)
      @lock.synchronize do
        # Re-inserting moves the route to the newest end of the Hash.
        @sandboxes[entrypoint] = @sandboxes.delete(entrypoint) || build(entrypoint, source)
      end
    end

    private

    def build(entrypoint, source)
      @sandboxes.delete(@sandboxes.each_key.first) if @sandboxes.size >= @size
      sandbox = Kobako::Sandbox.new(gvl: @gvl)
      sandbox.preload(code: source, name: entrypoint)
      sandbox
    end
  end

  # Rack-compatible application. Routes +GET /:name+ to +ROUTES[name]+ and
  # runs the matched entrypoint on its route's cached Sandbox. The root
  # path +/+ lists the available scripts so the demo is self-discoverable.
  class App
    PLAIN = { "content-type" => "text/plain" }.freeze

    def initialize(routes, cache)
      @routes = routes
      @cache = cache
    end

    def call(env)
      return method_not_allowed unless env["REQUEST_METHOD"] == "GET"

      name = env["PATH_INFO"].sub(%r{\A/}, "")
      return index if name.empty?

      entry = @routes[name]
      return not_found(name) unless entry

      invoke(entry, env)
    rescue Kobako::TrapError, Kobako::SandboxError, Kobako::ServiceError => e
      sandbox_error(e)
    end

    private

    # Each run gets its own guest, so requests sharing a Sandbox share no
    # guest state.
    def invoke(entry, rack_env)
      entrypoint, source = entry
      @cache.fetch(entrypoint, source).run(entrypoint, GuestRequest.new(rack_env)).value
    end

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
  end
end

if __FILE__ == $PROGRAM_NAME
  port = Integer(ENV.fetch("PORT", "9292"))
  host = ENV.fetch("HOST", "127.0.0.1")

  cache = Serverless::SandboxCache.new(size: options[:cache_size], gvl: options[:gvl].to_sym)
  app = Serverless::App.new(Serverless::ROUTES, cache)

  handler = Rackup::Handler.get(options[:type])
  warn "Serverless demo on http://#{host}:#{port} " \
       "(handler: #{handler.name}, gvl: #{options[:gvl]}, cache: #{options[:cache_size]})"
  handler.run(app, Host: host, Port: port)
end
