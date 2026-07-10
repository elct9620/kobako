# frozen_string_literal: true

# Shared setup for classes driving the production pure Guest Binary
# (`data/kobako.wasm`) through the public API — the end-to-end journeys
# under test/e2e/ (SPEC.md Testing Style Layer 4) and the pooled
# checkouts under test/pool/. On a clean checkout without the compiled
# ext or the built guest, each test skips with a pointer at the missing
# build step.
module E2eGuestHelper
  REAL_WASM = File.expand_path("../../data/kobako.wasm", __dir__)

  def setup
    # The default task compiles the ext and builds the guest before the
    # suite (`rake test` depends on `wasm:build`), so under CI a missing
    # prerequisite is a broken pipeline, never a skip — mirroring the
    # parity runner's cargo guard. A clean local checkout still skips.
    unless defined?(Kobako::Runtime)
      flunk "native ext not compiled under CI" if ENV["CI"]
      skip "native ext not compiled (run `bundle exec rake compile`)"
    end
    return if File.exist?(REAL_WASM)

    flunk "data/kobako.wasm missing under CI" if ENV["CI"]
    skip "data/kobako.wasm missing — run `bundle exec rake wasm:build`"
  end
end
