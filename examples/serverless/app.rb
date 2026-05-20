#!/usr/bin/env ruby
# frozen_string_literal: true

# Minimal serverless demo: a Rack app that dispatches GET /:name to an
# operator-provided mruby script. Each request constructs a fresh
# Kobako::Sandbox, preloads the script body as the :App entrypoint, and
# invokes it via #run(:App, env). The script returns a Rack response
# triplet [status, headers, body] directly.
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
  gem "kobako", "~> 0.3.0"
  gem "rack", "~> 3.0"
  gem "rackup", "~> 2.0"
  gem options[:type]
end

require "kobako"
require "rackup"
require "uri"

# Example types are nested under Serverless so the script has a single
# top-level constant — Rubocop's Style/OneClassPerFile is happy and the
# example reads top-down without splitting into multiple files.
module Serverless
  # Operator-managed script table — keys are the route segment after
  # `/`, values are mruby source that defines the `App` constant. The
  # script body is preloaded into a fresh +Kobako::Sandbox+ per request
  # and invoked as +App.call(env)+ where +env+ is a wire-friendly Hash
  # carrying request method, path, and query parameters.
  ROUTES = {
    "hello" => <<~'MRUBY',
      App = ->(env) {
        name = (env["query"]["name"] || "world")
        [200, { "content-type" => "text/plain" }, ["Hello, #{name}!\n"]]
      }
    MRUBY
    "echo" => <<~'MRUBY',
      App = ->(env) {
        body = "method: #{env["method"]}\npath: #{env["path"]}\nquery: #{env["query"].inspect}\n"
        [200, { "content-type" => "text/plain" }, [body]]
      }
    MRUBY
    "sum" => <<~'MRUBY',
      App = ->(env) {
        a = env["query"]["a"].to_i
        b = env["query"]["b"].to_i
        [200, { "content-type" => "text/plain" }, ["#{a} + #{b} = #{a + b}\n"]]
      }
    MRUBY
    "shout" => <<~'MRUBY'
      App = ->(env) {
        msg = env["query"]["msg"] || ""
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

      name = env["PATH_INFO"].to_s.sub(%r{\A/}, "")
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
      [200, PLAIN.dup, ["Available scripts:\n#{list}\n"]]
    end

    def not_found(name)
      [404, PLAIN.dup, ["no script registered for /#{name}\n"]]
    end

    def method_not_allowed
      [405, { "content-type" => "text/plain", "allow" => "GET" }, ["only GET is supported\n"]]
    end

    def sandbox_error(error)
      [500, PLAIN.dup, ["#{error.class}: #{error.message}\n"]]
    end

    # Build a fresh Sandbox per request, preload the script as :App, and
    # invoke it with the wire-friendly env Hash. The mruby triplet
    # +[status, headers, body]+ comes back through msgpack as plain
    # Integer / Hash / Array and is normalised so Rack's downstream
    # writers accept it without re-coercion.
    def render(script, rack_env)
      sandbox = Kobako::Sandbox.new
      sandbox.preload(code: script, name: :App)
      triplet = sandbox.run(:App, guest_env(rack_env))
      normalise(triplet)
    end

    # Whitelist Rack env keys that survive the wire codec. The mruby
    # guest only needs method / path / query — IO streams, middleware
    # callables, and host objects in the full Rack env are deliberately
    # excluded so the wire payload stays small and pure data.
    def guest_env(rack_env)
      {
        "method" => rack_env["REQUEST_METHOD"],
        "path" => rack_env["PATH_INFO"],
        "query" => parse_query(rack_env["QUERY_STRING"].to_s)
      }
    end

    def parse_query(query_string)
      return {} if query_string.empty?

      query_string.split("&").to_h do |pair|
        k, v = pair.split("=", 2)
        [URI.decode_www_form_component(k.to_s), URI.decode_www_form_component(v.to_s)]
      end
    end

    # Rack 3 requires Integer status, mutable Hash headers with
    # lowercase keys, and an Array body. The guest already produces
    # this shape, but we re-wrap the headers Hash so a frozen literal
    # from the script does not trip middleware that mutates headers.
    def normalise(triplet)
      status, headers, body = triplet
      [Integer(status), Hash(headers).dup, Array(body)]
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
