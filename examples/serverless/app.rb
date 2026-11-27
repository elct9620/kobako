#!/usr/bin/env ruby
# frozen_string_literal: true

# Minimal serverless demo: a Rack app that dispatches GET /:name to an
# operator-provided mruby script. The script body is preloaded as a named
# entrypoint constant and invoked via #run(:Entrypoint, GuestRequest.new(env)),
# returning a Rack response triplet [status, headers, body] directly.
#
# Each request builds a fresh Kobako::Sandbox, preloads only the matched
# route, and invokes it once; every #run runs a fresh mrb_state.
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
#
# Parsing CLI flags before bundler/inline runs is deliberate: only the
# server gem the operator picked is added to the inline Gemfile, so
# running with --type puma does not pull Falcon's dependency tree.

require "optparse"

SERVER_TYPES = %w[puma falcon].freeze

options = { type: "puma" }
OptionParser.new do |opts|
  opts.banner = "Usage: ruby examples/serverless/app.rb [options]"
  opts.on("--type TYPE", SERVER_TYPES,
          "Rack handler to start (#{SERVER_TYPES.join(", ")}; default: puma)") do |type|
    options[:type] = type
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

  # Rack-compatible application. Routes +GET /:name+ to +ROUTES[name]+ and
  # runs the matched entrypoint in a Sandbox of its own. The root path +/+
  # lists the available scripts so the demo is self-discoverable.
  class App
    PLAIN = { "content-type" => "text/plain" }.freeze

    def initialize(routes)
      @routes = routes
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

    # Builds a fresh Sandbox, preloads only the matched route's entrypoint,
    # and invokes it once, so concurrent requests share no guest state.
    def invoke(entry, rack_env)
      entrypoint, source = entry
      sandbox = Kobako::Sandbox.new
      sandbox.preload(code: source, name: entrypoint)
      sandbox.run(entrypoint, GuestRequest.new(rack_env)).value
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

  app = Serverless::App.new(Serverless::ROUTES)

  handler = Rackup::Handler.get(options[:type])
  warn "Serverless demo on http://#{host}:#{port} (handler: #{handler.name})"
  handler.run(app, Host: host, Port: port)
end
