# frozen_string_literal: true
#
# kobako Guest Binary boot script (mruby Ruby).
#
# This file is consumed by `wasm/kobako-wasm/src/rpc_client.rs` via
# `include_str!` and embedded into the Guest Binary as a static string.
# When the Guest Binary's `__kobako_run` reactor entry point reaches its
# mruby-side preload phase (item #11 wires this in), it evaluates this
# script with `mrb_load_string` to install the in-VM proxy infrastructure.
#
# Three responsibilities (SPEC.md "Outcome Envelope"; tmp/REFERENCE.md
# Ch.5 §Boot Script 三職責):
#
#   (1) Initialize the mruby-side state — capture `$stdout` / `$stderr`,
#       install error / panic handler hooks, ensure the Kobako module is
#       reachable. Lines 30 .. 53.
#
#   (2) Install the `Service::Group::Member` proxy — define `Kobako::RPC`
#       as a base class whose singleton `method_missing` routes every
#       class-level call through the Guest RPC Client (Rust bridge in
#       rpc_client.rs).  Each Service Member (e.g. `MyService::Logger`)
#       is, at runtime, a subclass of `Kobako::RPC`; the Frame 1 preamble
#       (Ch.5 §stdin 雙段 frame) creates these subclasses via mruby C API
#       — this script only installs the dispatch base class so the
#       subclasses inherit `method_missing`. Lines 55 .. 105.
#
#   (3) Drain stdout/stderr at end-of-run — leave WASI's stdout/stderr fds
#       untouched (they go straight to host-readable buffers per Ch.5
#       §third responsibility), but expose a `Kobako::Boot.flush_io`
#       helper the Rust outer driver calls right before writing the
#       outcome envelope. Lines 107 .. 122.
#
# This file is **mruby-syntactically constrained**: no pattern matching
# (`case ... in`), no rightward assignment (`expr => var`), no endless
# methods (`def f = expr`), no `Data.define`. mruby 3.2 (the pinned
# baseline per tmp/REFERENCE.md Ch.2) accepts everything in this script.

# ----------------------------------------------------------------------
# (1) Initialise mruby-side state.
# ----------------------------------------------------------------------
#
# Stash references to the original $stdout / $stderr so a misbehaving
# user script that reassigns them cannot break the post-execution drain.
# `Kobako::Boot::STDOUT_REF` / `STDERR_REF` are the canonical handles
# the third-responsibility hook reads from.
module Kobako
  module Boot
    STDOUT_REF = $stdout
    STDERR_REF = $stderr
  end
end

# ----------------------------------------------------------------------
# (2) Install the Service::Group::Member proxy.
# ----------------------------------------------------------------------
#
# `Kobako::RPC` is the dispatch base class. Each Service Member is a
# subclass of `Kobako::RPC` whose Ruby class name (e.g. "MyService::KV")
# *is* the wire-level target string. The C-API preamble in item #11
# defines these subclasses; this boot script is responsible for
# installing the singleton-class `method_missing` they inherit.
module Kobako
  class RPC
    class << self
      # All Service-Member class-level calls land here. `name` is the
      # method symbol; `args` are the positional arguments; if the last
      # arg is a Hash, it is treated as keyword arguments per Ruby
      # convention (REFERENCE Ch.5 §Boot Script 預載).
      def method_missing(name, *args, &_block)
        kwargs = {}
        if args.last.is_a?(Hash)
          # Keyword args: take the trailing Hash. Keys must be Strings on
          # the wire — coerce Symbol keys here (mruby user-script
          # convention writes Symbol keys).
          last = args.pop
          last.each_pair do |k, v|
            key = k.is_a?(Symbol) ? k.to_s : k.to_s
            kwargs[key] = v
          end
        end
        # `self` is the calling class object (e.g. MyService::KV). Its
        # `name` (e.g. "MyService::KV") is the wire-level target string.
        Kobako.__rpc_call__(self.name, name.to_s, args, kwargs)
      end

      # method_missing-paired hook: report responsiveness for any name so
      # mruby's `respond_to?` does not lie about what the proxy answers.
      def respond_to_missing?(_name, _include_private = false)
        true
      end
    end
  end

  # `Kobako::Handle` is the guest-side wrapper around a Capability Handle
  # (ext 0x01 wire form). Like `Kobako::RPC`, every method call is a
  # transparent RPC; the difference is the *target* — a Handle object
  # encodes on the wire as ext 0x01 rather than a Group::Member string.
  # The Rust bridge inspects `target` and dispatches accordingly.
  class Handle
    def method_missing(name, *args, &_block)
      kwargs = {}
      if args.last.is_a?(Hash)
        last = args.pop
        last.each_pair do |k, v|
          key = k.is_a?(Symbol) ? k.to_s : k.to_s
          kwargs[key] = v
        end
      end
      # Pass `self` (this Handle) as the target. The Rust bridge encodes
      # it as ext 0x01 in the Request envelope.
      Kobako.__rpc_call__(self, name.to_s, args, kwargs)
    end

    def respond_to_missing?(_name, _include_private = false)
      true
    end
  end
end

# ----------------------------------------------------------------------
# (3) End-of-run stdout / stderr drain hook.
# ----------------------------------------------------------------------
#
# WASI delivers fd 1 / fd 2 directly to the host's in-memory byte
# buffers (Ch.5 §third responsibility). Calling `flush_io` from the
# Rust driver immediately before writing the outcome envelope guarantees
# any buffered output reaches the host. The buffers themselves live in
# host memory; mruby never sees them.
module Kobako
  module Boot
    def self.flush_io
      STDOUT_REF.flush if STDOUT_REF.respond_to?(:flush)
      STDERR_REF.flush if STDERR_REF.respond_to?(:flush)
      nil
    end
  end
end
