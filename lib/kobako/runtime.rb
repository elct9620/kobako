# frozen_string_literal: true

module Kobako
  # The native wasmtime runtime, defined by the ext; this file adds only the
  # default Guest Binary path.
  class Runtime # :nodoc:
    # Absolute path to the gem-bundled +data/kobako.wasm+ artifact. Computed
    # from this file's location so it works for both +bundle exec+ (running
    # from the repo) and an installed gem (running from the gem dir).
    #
    # Returns a String regardless of whether the file currently exists —
    # call sites that need the file to be present should pass this through
    # +Kobako::Runtime.from_path+, which raises
    # +Kobako::ModuleNotBuiltError+ with a clear remediation message.
    # Raises +Kobako::SetupError+ if +__dir__+ is unavailable so that
    # +rescue Kobako::SetupError+ around +Kobako::Sandbox.new+ catches every
    # construction-layer failure uniformly, including this path-resolution one.
    def self.default_path
      dir = __dir__ or raise Kobako::SetupError, "Kobako::Runtime.default_path requires __dir__"
      File.expand_path("../../data/kobako.wasm", dir)
    end
  end
end
