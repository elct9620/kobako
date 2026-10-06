# frozen_string_literal: true

module Kobako
  # The native wasmtime runtime, defined by the ext; this file adds only the
  # default Guest Binary path.
  class Runtime # :nodoc:
    # The path is answered whether or not the file exists; from_path is
    # where a missing artifact is refused. A missing +__dir__+ raises
    # SetupError so one rescue around Sandbox.new covers every construction
    # failure.
    def self.default_path
      dir = __dir__ or raise Kobako::SetupError, "Kobako::Runtime.default_path requires __dir__"
      File.expand_path("../../data/kobako.wasm", dir)
    end
  end
end
