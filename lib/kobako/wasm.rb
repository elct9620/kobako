# frozen_string_literal: true

module Kobako
  # Host-side wasmtime wrapper, surfaced as Ruby classes by the native ext
  # (see ext/kobako/src/wasm.rs). This module is the foundational binding
  # layer for Sandbox (#14), the run path (#16) and RPC dispatch (#18).
  #
  # The classes themselves (Engine / Module / Store / Instance) and the
  # error hierarchy (Error / ModuleNotBuiltError) are defined from Rust at
  # ext load time; this file only adds the pure-Ruby helpers that have no
  # reason to live in Rust.
  module Wasm
    # Absolute path to the gem-bundled `data/kobako.wasm` artifact. Computed
    # from this file's location so it works for both `bundle exec` (running
    # from the repo) and an installed gem (running from the gem dir).
    #
    # Returns a String regardless of whether the file currently exists —
    # call sites that need the file to be present should pass this through
    # `Kobako::Wasm::Module.from_file`, which raises `ModuleNotBuiltError`
    # with a clear remediation message.
    def self.default_path
      File.expand_path("../../data/kobako.wasm", __dir__)
    end

    # Unpack the +(ptr << 32) | len+ u64 produced by the Rust ext's
    # +__kobako_take_outcome+ export. Returns +[ptr, len]+ as 32-bit
    # unsigned integers. Pure-Ruby helper kept near the ABI surface so
    # Sandbox does not have to carry bit-level wire layout.
    def self.unpack_outcome_ptr_len(packed)
      ptr = (packed >> 32) & 0xffff_ffff
      len = packed & 0xffff_ffff
      [ptr, len]
    end
  end
end
