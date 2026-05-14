#!/usr/bin/env ruby
# frozen_string_literal: true

# Minimal CodeMode REPL: ruby_llm drives a chat that can call an `execute`
# tool, which runs mruby inside a Kobako::Sandbox bound to a shared KV
# store and a WebFetch client. Outbound HTTP is gated by an in-memory
# domain allowlist that the operator mutates from the REPL via
# `/allow` and `/disallow` slash commands.

require "bundler/inline"

gemfile do
  source "https://rubygems.org"
  gem "ruby_llm"
  gem "reline"
  gem "kobako", path: File.expand_path("../..", __dir__)
end

require "kobako"
require "ruby_llm"
require "reline"
require "net/http"
require "uri"

# Example types are nested under CodeMode so the script has a single
# top-level constant — Rubocop's Style/OneClassPerFile is happy and the
# example reads top-down without splitting into multiple files.
module CodeMode
  # Thread-safe in-memory key-value store exposed to the sandbox as KV::Store.
  class KvStore
    def initialize
      @mutex = Mutex.new
      @store = {}
    end

    def get(key)
      @mutex.synchronize { @store[key.to_s] }
    end

    def set(key, value)
      @mutex.synchronize { @store[key.to_s] = value }
    end

    def delete(key)
      @mutex.synchronize { @store.delete(key.to_s) }
    end

    def keys
      @mutex.synchronize { @store.keys }
    end
  end

  # Mutable, thread-safe set of hostnames the WebFetch binding is permitted
  # to contact. Empty by default — the operator must explicitly add hosts
  # from the REPL with `/allow`, so an untrusted script cannot exfiltrate
  # data to an arbitrary endpoint just because it can construct a URL.
  class DomainAllowlist
    def initialize
      @mutex = Mutex.new
      @hosts = Set.new
    end

    def allow(host)
      normalised = normalise(host)
      added = @mutex.synchronize { @hosts.add?(normalised) }
      { host: normalised, changed: !added.nil? }
    end

    def disallow(host)
      normalised = normalise(host)
      removed = @mutex.synchronize { @hosts.delete?(normalised) }
      { host: normalised, changed: !removed.nil? }
    end

    def allowed?(host)
      @mutex.synchronize { @hosts.include?(normalise(host)) }
    end

    def list
      @mutex.synchronize { @hosts.to_a.sort }
    end

    private

    def normalise(host)
      host.to_s.strip.downcase
    end
  end

  # Service object bound to the sandbox as `WebFetch::Client`. Issues a
  # single GET against +url+ and returns a wire-friendly Hash. Failures
  # (disallowed host, bad scheme, oversized body, timeout) raise on the
  # host side; the Registry::Dispatcher reifies those as Response.err
  # envelopes so the guest sees a normal Ruby exception.
  class WebFetchClient
    MAX_BODY_BYTES = 512 * 1024
    TIMEOUT_SECONDS = 10
    ALLOWED_SCHEMES = %w[http https].freeze

    def initialize(allowlist)
      @allowlist = allowlist
    end

    def get(url)
      uri = parse_url(url)
      enforce_allowlist!(uri)
      perform_get(uri)
    end

    private

    def parse_url(url)
      uri = URI.parse(url.to_s)
      unless ALLOWED_SCHEMES.include?(uri.scheme)
        raise ArgumentError,
              "unsupported scheme #{uri.scheme.inspect} (allowed: #{ALLOWED_SCHEMES.join(", ")})"
      end
      raise ArgumentError, "url has no host: #{url.inspect}" if uri.host.nil? || uri.host.empty?

      uri
    rescue URI::InvalidURIError => e
      raise ArgumentError, "invalid url: #{e.message}"
    end

    def enforce_allowlist!(uri)
      host = uri.host.downcase
      return if @allowlist.allowed?(host)

      raise "domain not allowed: #{host} (operator must `/allow #{host}` in the REPL first)"
    end

    def perform_get(uri)
      Net::HTTP.start(uri.host, uri.port,
                      use_ssl: uri.scheme == "https",
                      open_timeout: TIMEOUT_SECONDS,
                      read_timeout: TIMEOUT_SECONDS) do |http|
        http.request(Net::HTTP::Get.new(uri.request_uri)) do |response|
          return build_result(response, read_capped_body(response))
        end
      end
    end

    def read_capped_body(response)
      buffer = +""
      response.read_body do |chunk|
        buffer << chunk
        raise "response body exceeds #{MAX_BODY_BYTES} bytes" if buffer.bytesize > MAX_BODY_BYTES
      end
      buffer
    end

    def build_result(response, body)
      {
        status: response.code.to_i,
        headers: response.to_hash.transform_values { |values| values.join(", ") },
        body: body
      }
    end
  end

  SYSTEM_PROMPT = <<~PROMPT
    You can run mruby code in a Wasm sandbox by calling the tool
    `execute(code:)`. Inside the script the constants `KV::Store` and
    `WebFetch::Client` are available; their APIs in RBS are:

        module KV
          module Store
            def self.get: (String key) -> untyped
            def self.set: (String key, untyped value) -> untyped
            def self.delete: (String key) -> untyped
            def self.keys: () -> Array[String]
          end
        end

        module WebFetch
          module Client
            def self.get: (String url) -> { status: Integer,
                                             headers: Hash[String, String],
                                             body: String }
          end
        end

    The KV store persists between tool calls. `WebFetch::Client.get` only
    accepts http/https URLs whose host is on the operator's allowlist;
    other URLs raise. If a fetch you need is blocked, surface the host
    in your reply so the operator can decide whether to `/allow` it.

    Prefer scripts that return or print their result so you can read it
    back from the tool output.
  PROMPT

  # RubyLLM tool: evaluate mruby code in an injected sandbox.
  class Execute < RubyLLM::Tool
    description <<~DESC
      Evaluate mruby code inside an isolated Wasm sandbox and return the
      last expression along with captured stdout and stderr. The sandbox
      is reused across calls, so values stored in KV::Store persist.
    DESC

    param :code, type: :string, desc: "mruby source code to evaluate"

    def initialize(sandbox)
      super()
      @sandbox = sandbox
    end

    def execute(code:)
      result = @sandbox.run(code)
      { result: result.inspect, stdout: @sandbox.stdout, stderr: @sandbox.stderr }
    rescue Kobako::SandboxError, Kobako::ServiceError, Kobako::TrapError => e
      { error: "#{e.class}: #{e.message}" }
    end
  end

  # Human-readable, colourised tool-call trace. Wired into the chat via
  # ruby_llm's official +on_tool_call+ / +on_tool_result+ callbacks so we
  # render the agent's dispatch into the sandbox without monkey-patching.
  # ANSI is skipped when stdout is not a TTY so piping the script to a
  # file stays clean.
  module Trace
    PALETTE = {
      reset: "\e[0m", dim: "\e[2m", bold: "\e[1m",
      cyan: "\e[36m", green: "\e[32m", red: "\e[31m"
    }.freeze

    module_function

    def paint(text, color)
      return text.to_s unless $stdout.tty?

      "#{PALETTE[color]}#{text}#{PALETTE[:reset]}"
    end

    def tool_call(call)
      puts "#{paint("→ tool", :cyan)} #{paint(call.name, :bold)}"
      call.arguments.each { |key, value| print_field(key, value, :dim) }
    end

    def tool_result(tool_name, result)
      puts "#{paint("← tool", :cyan)} #{paint(tool_name, :bold)}"
      return print_field(:value, result, :dim) unless result.is_a?(Hash)

      result.each do |key, value|
        next if value.nil? || (value.respond_to?(:empty?) && value.empty?)

        color = { result: :green, error: :red }.fetch(key, :dim)
        print_field(key, value, color)
      end
    end

    def print_field(key, value, color)
      puts paint("  #{key}:", color)
      value.to_s.each_line { |line| puts paint("    #{line.chomp}", color) }
    end
  end

  # Slash-command dispatcher for the REPL. Lines beginning with `/`
  # never reach the LLM — they belong to the operator, not the agent.
  # `#handle` returns `:exit` to terminate the loop, `:handled` when a
  # command was recognised (or rejected), and `:passthrough` so the
  # caller forwards the line to `chat.ask`.
  module ReplCommands
    HELP = <<~HELP
      Commands:
        /allow [host …]      Add hosts to the WebFetch allowlist (no args lists current)
        /disallow [host …]   Remove hosts from the WebFetch allowlist
        /help                Show this help
        /exit                Quit the REPL
    HELP

    module_function

    def handle(input, allowlist)
      return :passthrough unless input.start_with?("/")

      verb, *args = input.split
      case verb.downcase
      when "/exit"     then :exit
      when "/help"     then puts(HELP) || :handled
      when "/allow"    then handle_allow(args, allowlist)
      when "/disallow" then handle_disallow(args, allowlist)
      else                  unknown(verb)
      end
    end

    def unknown(verb)
      warn "unknown command: #{verb}. Type /help for the list."
      :handled
    end

    def handle_allow(args, allowlist)
      return print_allowlist(allowlist) if args.empty?

      args.each do |raw|
        result = allowlist.allow(raw)
        puts(result[:changed] ? "allowed: #{result[:host]}" : "already allowed: #{result[:host]}")
      end
      :handled
    end

    def handle_disallow(args, allowlist)
      if args.empty?
        warn "usage: /disallow <host> [host …]"
        return :handled
      end

      args.each do |raw|
        result = allowlist.disallow(raw)
        puts(result[:changed] ? "removed: #{result[:host]}" : "not allowed: #{result[:host]}")
      end
      :handled
    end

    def print_allowlist(allowlist)
      hosts = allowlist.list
      puts(hosts.empty? ? "no hosts allowed (use /allow <host>)" : "allowed: #{hosts.join(", ")}")
      :handled
    end
  end
end

RubyLLM.configure do |config|
  config.openai_api_key = ENV.fetch("OPENAI_API_KEY", "dummy")
  base = ENV.fetch("OPENAI_BASE_URL", nil)
  config.openai_api_base = base if base && !base.empty?

  # Tool activity is rendered by CodeMode::Trace; suppress ruby_llm's
  # raw debug stream so the colourised trace is the only signal on the
  # console. Warnings still surface at this level.
  config.log_level = Logger::WARN
end

sandbox = Kobako::Sandbox.new
sandbox.define(:KV).bind(:Store, CodeMode::KvStore.new)
allowlist = CodeMode::DomainAllowlist.new
sandbox.define(:WebFetch).bind(:Client, CodeMode::WebFetchClient.new(allowlist))

model = ENV.fetch("OPENAI_DEFAULT_MODEL", "gpt-5.4-mini")
chat = RubyLLM
       .chat(model: model, provider: :openai, assume_model_exists: true)
       .with_instructions(CodeMode::SYSTEM_PROMPT)
       .with_tool(CodeMode::Execute.new(sandbox))

# Shared between on_tool_call/on_tool_result so the result line can
# echo the tool name — ruby_llm's tool_result callback only carries
# the return value, and the two callbacks always fire in a pair per
# tool invocation, so a local closure variable is enough.
last_tool_name = nil
chat.on_tool_call do |call|
  last_tool_name = call.name
  CodeMode::Trace.tool_call(call)
end
chat.on_tool_result { |result| CodeMode::Trace.tool_result(last_tool_name, result) }

puts "Kobako CodeMode — model=#{model}  (type /help for commands, /exit to quit)"

loop do
  line = Reline.readline("you> ", true)
  break if line.nil?

  line = line.strip
  next if line.empty?

  case CodeMode::ReplCommands.handle(line, allowlist)
  when :exit then break
  when :handled then next
  end

  begin
    response = chat.ask(line)
    puts "assistant> #{response.content}"
  rescue StandardError => e
    warn "[error] #{e.class}: #{e.message}"
  end
end
