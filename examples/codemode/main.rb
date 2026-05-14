#!/usr/bin/env ruby
# frozen_string_literal: true

# Minimal CodeMode REPL: ruby_llm drives a chat that can call an `execute`
# tool, which runs mruby inside a Kobako::Sandbox bound to a shared KV store.

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

  SYSTEM_PROMPT = <<~PROMPT
    You can run mruby code in a Wasm sandbox by calling the tool
    `execute(code:)`. Inside the script the constant `KV::Store` is
    available; its API in RBS is:

        module KV
          module Store
            def self.get: (String key) -> untyped
            def self.set: (String key, untyped value) -> untyped
            def self.delete: (String key) -> untyped
            def self.keys: () -> Array[String]
          end
        end

    The store persists between tool calls. Prefer scripts that return or
    print their result so you can read it back from the tool output.
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

puts "Kobako CodeMode — model=#{model}  (type /exit to quit)"

loop do
  line = Reline.readline("you> ", true)
  break if line.nil?

  line = line.strip
  next if line.empty?
  break if line == "/exit"

  begin
    response = chat.ask(line)
    puts "assistant> #{response.content}"
  rescue StandardError => e
    warn "[error] #{e.class}: #{e.message}"
  end
end
