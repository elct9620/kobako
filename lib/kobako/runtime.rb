# frozen_string_literal: true

module Kobako
  # Host-side wasmtime runtime, surfaced as a Ruby class by the native ext
  # (see ext/kobako/src/runtime.rs). The +Kobako::Runtime+ class wraps the
  # wasmtime engine + compiled module + Store; it is the only Ruby-visible
  # wasmtime type and the foundational binding layer for +Kobako::Sandbox+.
  #
  # This file reopens the magnus-defined class only to add the pure-Ruby
  # +.default_path+ helper. Every other method (+#from_path+ singleton,
  # +#eval+ / +#run+, capture and usage readers) is registered from Rust
  # at ext load time.
  class Runtime
    # Absolute path to the gem-bundled +data/kobako.wasm+ artifact. Computed
    # from this file's location so it works for both +bundle exec+ (running
    # from the repo) and an installed gem (running from the gem dir).
    #
    # Returns a String regardless of whether the file currently exists —
    # call sites that need the file to be present should pass this through
    # +Kobako::Runtime.from_path+, which raises
    # +Kobako::ModuleNotBuiltError+ with a clear remediation message.
    def self.default_path
      dir = __dir__ or raise Kobako::Error, "Kobako::Runtime.default_path requires __dir__"
      File.expand_path("../../data/kobako.wasm", dir)
    end
  end
end
