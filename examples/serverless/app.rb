#!/usr/bin/env ruby
# frozen_string_literal: true

# Minimal serverless demo: a Rack app that dispatches GET /:name to an
# operator-provided mruby script. Each request constructs a fresh
# Kobako::Sandbox, preloads the script body as the :App entrypoint, and
# invokes it via #run(:App, Rack::Request.new(env)). The script returns
# a Rack response triplet [status, headers, body] directly.
#
# The Rack::Request is a non-wire-representable host object, so kobako
# 0.4.0's #run host→guest auto-wrap (SPEC B-34) allocates a Handle for
# it; the guest receives a Kobako::Handle proxy whose method calls
# (request_method, path, params) round-trip back through RPC. No
# host-side marshalling step sits between the Rack env and the script.
#
# Usage:
#   ruby examples/serverless/app.rb                # Puma (default)
#   ruby examples/serverless/app.rb --type puma
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
  gem "kobako", "~> 0.4.0"
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
  # Operator-managed script table — keys are the route segment after
  # `/`, values are mruby source that defines the `App` constant. The
  # script body is preloaded into a fresh +Kobako::Sandbox+ per request
  # and invoked as +App.call(req)+ where +req+ is a +Kobako::Handle+
  # proxy of a host-side +Rack::Request+: every +req.params+ /
  # +req.request_method+ / +req.path+ call dispatches back to the host
  # as one RPC round-trip.
  ROUTES = {
    "hello" => <<~'MRUBY',
      App = ->(req) {
        name = (req.params["name"] || "world")
        [200, { "content-type" => "text/plain" }, ["Hello, #{name}!\n"]]
      }
    MRUBY
    "echo" => <<~'MRUBY',
      App = ->(req) {
        body = "method: #{req.request_method}\npath: #{req.path}\nquery: #{req.params.inspect}\n"
        [200, { "content-type" => "text/plain" }, [body]]
      }
    MRUBY
    "sum" => <<~'MRUBY',
      App = ->(req) {
        a = req.params["a"].to_i
        b = req.params["b"].to_i
        [200, { "content-type" => "text/plain" }, ["#{a} + #{b} = #{a + b}\n"]]
      }
    MRUBY
    "shout" => <<~'MRUBY'
      App = ->(req) {
        msg = req.params["msg"] || ""
        [200, { "content-type" => "text/plain" }, ["#{msg.upcase}!\n"]]
      }
    MRUBY
  }.freeze

  # Rack-compatible application. Routes +GET /:name+ to +ROUTES[name]+,
  # spinning up a fresh +Kobako::Sandbox+ per request so concurrent
  # invocations cannot share guest state. The root path +/+ lists the
  # available scripts so the demo is self-discoverable.
  class App
    PLAIN = { "content-type" => "text/plain" }.freeze

    def initialize(routes)
      @routes = routes
    end

    def call(env)
      return method_not_allowed unless env["REQUEST_METHOD"] == "GET"

      name = env["PATH_INFO"].sub(%r{\A/}, "")
      return index if name.empty?

      script = @routes[name]
      return not_found(name) unless script

      render(script, env)
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

    # Build a fresh Sandbox per request, preload the script as :App, and
    # invoke it with a +Rack::Request+ wrapping the Rack env. The Request
    # is not wire-representable, so kobako auto-wraps it into a Handle
    # (SPEC B-34) and the guest sees a +Kobako::Handle+ proxy; method
    # calls on +req+ inside the script dispatch back to the host as RPC,
    # so query parsing and header access happen on the host's full Rack
    # implementation. The returned +[status, headers, body]+ triplet
    # comes back through msgpack as plain mutable Integer / Hash /
    # Array, which is exactly what Rack 3 wants.
    def render(script, rack_env)
      sandbox = Kobako::Sandbox.new
      sandbox.preload(code: script, name: :App)
      sandbox.run(:App, Rack::Request.new(rack_env))
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
