# CodeMode REPL

A self-contained example that wires [ruby_llm](https://rubygems.org/gems/ruby_llm) to a `Kobako::Sandbox` so a chat agent can run mruby code through a single `execute(code:)` tool. The sandbox is preloaded with a shared in-memory KV store and a `WebFetch::Client`; outbound HTTP is gated by an operator-controlled domain allowlist that you mutate from the REPL.

This is the canonical demonstration of the "code mode" pattern: the model writes a short script instead of orchestrating many narrow tool calls, and every script runs inside the same Wasm-isolated mruby interpreter the host process embeds.

## Running

The script uses `bundler/inline`, so it resolves its own dependencies on first run — no `Gemfile` is required in the working directory.

```bash
ruby examples/codemode/main.rb
```

From a clone of the kobako repository:

```bash
bundle exec ruby examples/codemode/main.rb
```

First launch downloads `ruby_llm`, `reline`, and `kobako` (~0.19). Subsequent launches reuse the resolved set.

## Configuration

The example talks to OpenAI by default. Three environment variables control where the request goes and which model handles it.

| Variable               | Purpose                                                                 | Default       |
|------------------------|-------------------------------------------------------------------------|---------------|
| `OPENAI_API_KEY`       | Bearer credential sent to the endpoint.                                 | `dummy`       |
| `OPENAI_BASE_URL`      | Override the API base URL. Leave unset to use OpenAI's hosted endpoint. | _(unset)_     |
| `OPENAI_DEFAULT_MODEL` | Model identifier passed to `RubyLLM.chat(model:)`.                      | `gpt-5.4-mini`|

The chat is constructed with `provider: :openai, assume_model_exists: true`, so any endpoint that speaks the OpenAI Chat Completions protocol works — the model string is forwarded verbatim without a local registry lookup.

### Switching provider via OpenAI-compatible endpoints

Most providers ship an OpenAI-compatible surface. Point `OPENAI_BASE_URL` at it and set `OPENAI_DEFAULT_MODEL` to a model that endpoint serves.

```bash
# OpenAI (default — no overrides needed beyond the key)
OPENAI_API_KEY=sk-... \
  ruby examples/codemode/main.rb

# Ollama (local, no auth)
OPENAI_BASE_URL=http://localhost:11434/v1 \
OPENAI_DEFAULT_MODEL=llama3.1 \
  ruby examples/codemode/main.rb

# LM Studio (local)
OPENAI_BASE_URL=http://localhost:1234/v1 \
OPENAI_DEFAULT_MODEL=qwen2.5-coder-7b-instruct \
  ruby examples/codemode/main.rb

# OpenRouter
OPENAI_API_KEY=sk-or-... \
OPENAI_BASE_URL=https://openrouter.ai/api/v1 \
OPENAI_DEFAULT_MODEL=anthropic/claude-sonnet-4.5 \
  ruby examples/codemode/main.rb

# Groq
OPENAI_API_KEY=gsk_... \
OPENAI_BASE_URL=https://api.groq.com/openai/v1 \
OPENAI_DEFAULT_MODEL=llama-3.3-70b-versatile \
  ruby examples/codemode/main.rb
```

Endpoints without an OpenAI-compatible surface (e.g. Anthropic's native API, Google's Gemini API) require changing the `provider:` argument in `main.rb` and configuring the matching `ruby_llm` provider block. The example intentionally keeps a single provider wired up; treat it as a starting point.

## What the agent can do

Once the REPL is running, the system prompt tells the model the sandbox exposes two host modules. They are visible only inside the `execute(code:)` tool — the script writes mruby that calls into them.

```ruby
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
```

`KV::Store` persists across tool calls within one REPL session. `WebFetch::Client.get` only succeeds when the target host has been added to the allowlist from the REPL (see below); other URLs raise back to the agent so it can ask the operator to allow the host.

## REPL commands

Lines beginning with `/` are intercepted before reaching the model.

| Command              | Effect                                                    |
|----------------------|-----------------------------------------------------------|
| `/allow [host …]`    | Add hosts to the WebFetch allowlist. No args lists current. |
| `/disallow host …`   | Remove hosts from the allowlist.                          |
| `/help`              | Show the command list.                                    |
| `/exit`              | Quit the REPL. `Ctrl-D` also works.                       |

Tab completion expands `/` prefixes; the leading verb is coloured cyan when recognised, red otherwise.

## Security caveats

The `WebFetch::Client` performs a **textual** hostname match against the allowlist. Production deployments must layer on IP-level egress controls (block link-local, RFC1918, cloud metadata endpoints) and DNS-rebind protection — an allowed name can still resolve to an internal address between the allowlist check and the TCP connect. Treat this example as a teaching aid, not a hardened gateway.
