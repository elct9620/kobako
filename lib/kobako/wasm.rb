# frozen_string_literal: true

module Kobako
  # Host-side wasmtime wrapper, surfaced as Ruby classes by the native ext
  # (see ext/kobako/src/wasm.rs). This module is the foundational binding
  # layer for Sandbox (#14), the run path (#16) and RPC dispatch (#18).
  #
  # The classes themselves (Instance) and the error hierarchy (Error /
  # ModuleNotBuiltError) are defined from Rust at ext load time; this file
  # only adds the pure-Ruby helpers that have no reason to live in Rust.
  module Wasm
    # Absolute path to the gem-bundled `data/kobako.wasm` artifact. Computed
    # from this file's location so it works for both `bundle exec` (running
    # from the repo) and an installed gem (running from the gem dir).
    #
    # Returns a String regardless of whether the file currently exists —
    # call sites that need the file to be present should pass this through
    # +Kobako::Wasm::Instance.from_path+, which raises +ModuleNotBuiltError+
    # with a clear remediation message.
    def self.default_path
      dir = __dir__ or raise Error, "Kobako::Wasm.default_path requires __dir__"
      File.expand_path("../../data/kobako.wasm", dir)
    end
  end
end
