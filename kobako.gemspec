# frozen_string_literal: true

require_relative "lib/kobako/version"

Gem::Specification.new do |spec|
  spec.name = "kobako"
  spec.version = Kobako::VERSION
  spec.authors = ["Aotokitsuruya"]
  spec.email = ["contact@aotoki.me"]

  spec.summary = "Embeddable Wasm sandbox for running untrusted mruby code from Ruby applications."
  spec.description = "kobako provides an in-process Wasm sandbox (wasmtime + mruby) with a MessagePack-based host/guest RPC, allowing Ruby applications to execute untrusted mruby scripts under capability-based Service injection."
  spec.homepage = "https://github.com/elct9620/kobako"
  spec.license = "Apache-2.0"
  spec.required_ruby_version = ">= 3.3.0"
  spec.required_rubygems_version = ">= 3.3.11"

  spec.metadata["allowed_push_host"] = "https://rubygems.org"
  spec.metadata["homepage_uri"] = spec.homepage
  spec.metadata["source_code_uri"] = "https://github.com/elct9620/kobako"
  spec.metadata["changelog_uri"] = "https://github.com/elct9620/kobako/blob/main/CHANGELOG.md"
  spec.metadata["bug_tracker_uri"] = "https://github.com/elct9620/kobako/issues"
  spec.metadata["rubygems_mfa_required"] = "true"

  # Specify which files should be added to the gem when it is released.
  # The `git ls-files -z` loads the files in the RubyGem that have been added into git.
  # `data/kobako.wasm` is gitignored (built by `rake wasm:build`) so it is
  # appended explicitly when present.
  #
  # The deny prefixes exclude dev-only / non-runtime artifacts:
  #   * source-tree tooling: bin/ tasks/ build_config/ .github/ .powerloop/
  #     .claude/ Rakefile .rubocop.yml Steepfile rbs_collection.yaml
  #     rbs_collection.lock.yaml — `gem install` uses extconf.rb, not rake
  #   * non-runtime content: test/ wasm/ docs/ benchmark/ examples/ SPEC.md CLAUDE.md
  #   * placeholder: data/.keep — superseded by the appended data/kobako.wasm
  #
  # `sig/` is intentionally **kept** so downstream gems can consume kobako's
  # RBS via Steep / RBS tooling. (The steep gem itself excludes sig/ because
  # it self-hosts; rbs ships its sig/. We follow the rbs convention.)
  # `sig/_external/` holds dev-only stubs for upstream gems whose
  # maintainers have not yet published RBS — kept out of the gem so
  # downstream consumers are not handed our partial third-party signatures.
  gemspec = File.basename(__FILE__)
  spec.files = IO.popen(%w[git ls-files -z], chdir: __dir__, err: IO::NULL) do |ls|
    ls.readlines("\x0", chomp: true).reject do |f|
      (f == gemspec) ||
        f.start_with?(*%w[bin/ Gemfile Gemfile.lock .gitignore test/ .github/ .rubocop.yml
                          tasks/ build_config/ wasm/ docs/ benchmark/ examples/ .powerloop/
                          SPEC.md .claude/ CLAUDE.md Rakefile Steepfile rbs_collection.yaml
                          rbs_collection.lock.yaml sig/_external/ data/.keep])
    end
  end
  # Bypass-path guard: `gem build kobako.gemspec` direct invocation would
  # silently package a gem without the Guest Binary. The warn is gated to
  # the `gem` program so it doesn't fire during rake's eager gemspec load.
  wasm_path = File.join(__dir__, "data/kobako.wasm")
  if File.exist?(wasm_path)
    spec.files += ["data/kobako.wasm"]
  elsif File.basename($PROGRAM_NAME) == "gem"
    warn "kobako.gemspec: data/kobako.wasm is absent — run " \
         "`bundle exec rake wasm:build` before `gem build`."
  end
  spec.bindir = "exe"
  spec.executables = []
  spec.require_paths = ["lib"]
  spec.extensions = ["ext/kobako/extconf.rb"]

  # MessagePack codec backbone for the host side of the kobako wire (SPEC.md
  # "Wire Codec"). The Host Gem registers ext type 0x01 (Capability Handle)
  # and ext type 0x02 (Exception envelope) on a `MessagePack::Factory`; the
  # gem's hand-written byte-level encoder/decoder has been retired.
  spec.add_dependency "msgpack", "~> 1.7"
  spec.add_dependency "rb_sys", "~> 0.9.91"

  # For more information and examples about making a new gem, check out our
  # guide at: https://bundler.io/guides/creating_gem.html
end
