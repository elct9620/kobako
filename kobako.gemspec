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
  spec.required_ruby_version = ">= 3.2.0"
  spec.required_rubygems_version = ">= 3.3.11"

  spec.metadata["allowed_push_host"] = "https://rubygems.org"
  spec.metadata["homepage_uri"] = spec.homepage
  spec.metadata["source_code_uri"] = "https://github.com/elct9620/kobako"
  spec.metadata["changelog_uri"] = "https://github.com/elct9620/kobako/blob/main/CHANGELOG.md"
  spec.metadata["bug_tracker_uri"] = "https://github.com/elct9620/kobako/issues"
  spec.metadata["rubygems_mfa_required"] = "true"

  # Specify which files should be added to the gem when it is released.
  # The `git ls-files -z` loads the files in the RubyGem that have been added into git.
  # `data/kobako.wasm` is gitignored (built by `rake wasm:guest`) so it is
  # appended explicitly when present.
  gemspec = File.basename(__FILE__)
  spec.files = IO.popen(%w[git ls-files -z], chdir: __dir__, err: IO::NULL) do |ls|
    ls.readlines("\x0", chomp: true).reject do |f|
      (f == gemspec) ||
        f.start_with?(*%w[bin/ Gemfile Gemfile.lock .gitignore test/ .github/ .rubocop.yml
                          tasks/ build_config/ wasm/ docs/ benchmark/ .powerloop/ SPEC.md])
    end
  end
  spec.files += ["data/kobako.wasm"] if File.exist?(File.join(__dir__, "data/kobako.wasm"))
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
