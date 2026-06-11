# frozen_string_literal: true

# Shared setup for the end-to-end journey classes under test/e2e/ (SPEC.md
# Testing Style Layer 4). Every scenario drives the production pure Guest
# Binary (`data/kobako.wasm`) through the public Sandbox API; on a clean
# checkout without the compiled ext or the built guest, each test skips
# with a pointer at the missing build step.
module E2eGuestHelper
  REAL_WASM = File.expand_path("../../data/kobako.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    return if File.exist?(REAL_WASM)

    skip "data/kobako.wasm missing — run `bundle exec rake wasm:build` " \
         "(requires `rake vendor:setup` + `rake mruby:build` first)"
  end
end
