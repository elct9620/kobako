# frozen_string_literal: true

# Shared setup for classes driving the production pure Guest Binary
# (`data/kobako.wasm`) through the public API — the end-to-end journeys
# under test/e2e/. On a clean checkout without the compiled
# ext or the built guest, each test skips with a pointer at the missing
# build step.
module E2eGuestHelper
  include GuestGuard

  REAL_WASM = TestPaths.data("kobako.wasm")

  def setup
    require_guest_binary(REAL_WASM, build: "bundle exec rake wasm:build")
  end
end
