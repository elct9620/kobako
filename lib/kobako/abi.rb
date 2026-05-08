# frozen_string_literal: true

# Wire ABI surface — host-side mirror of the wasm import/export contract.
#
# This module documents the contract pinned by SPEC.md "ABI Signatures".
# The Rust guest crate (`wasm/kobako-wasm/src/abi.rs`) declares the same
# names and packed-u64 formula. Both sides must agree byte-for-byte.
#
# Like +lib/kobako/wire.rb+, this module is intentionally self-contained
# — it does not load the native extension — so item #12 (host wasmtime
# wiring) and the build-pipeline guard (item #26) can require it from a
# clean checkout.
#
# See SPEC.md → Wire Codec → ABI Signatures for the binary layout.
module Kobako
  # Wire ABI constants and packed-u64 helpers shared by host and guest.
  #
  # SPEC pins:
  #
  # * Exactly 1 host import: +__kobako_rpc_call+ in the +env+ wasm namespace,
  #   signature +(req_ptr: i32, req_len: i32) -> i64+.
  # * Exactly 3 guest exports:
  #   - +__kobako_run+              — +() -> ()+
  #   - +__kobako_alloc+            — +(size: i32) -> i32+
  #   - +__kobako_take_outcome+     — +() -> i64+
  # * Packed u64 layout (used by +__kobako_rpc_call+ and
  #   +__kobako_take_outcome+): high 32 bits = ptr, low 32 bits = len.
  module ABI
    # Wasm namespace the host import lives in.
    IMPORT_MODULE = "env"

    # Sole host-provided import function name.
    IMPORT_NAME = "__kobako_rpc_call"

    # All three guest-provided export names, in declaration order.
    EXPORT_NAMES = %w[
      __kobako_run
      __kobako_alloc
      __kobako_take_outcome
    ].freeze

    # 32-bit unsigned mask for the low half of a packed u64.
    U32_MASK = 0xffff_ffff

    module_function

    # Pack +(ptr, len)+ into a single 64-bit unsigned integer where the high
    # 32 bits hold +ptr+ and the low 32 bits hold +len+. Mirrors
    # +pack_u64+ in +wasm/kobako-wasm/src/abi.rs+.
    #
    # @param ptr [Integer] wasm linear memory offset (0..2**32-1)
    # @param len [Integer] byte length (0..2**32-1)
    # @return [Integer] packed u64
    # @raise [ArgumentError] if either operand falls outside the u32 range
    def pack_u64(ptr, len)
      raise ArgumentError, "ptr out of u32 range: #{ptr}" unless ptr.between?(0, U32_MASK)
      raise ArgumentError, "len out of u32 range: #{len}" unless len.between?(0, U32_MASK)

      (ptr << 32) | len
    end

    # Inverse of {pack_u64}. Returns +[ptr, len]+.
    #
    # @param packed [Integer] packed u64 produced by {pack_u64}
    # @return [Array(Integer, Integer)] +(ptr, len)+
    # @raise [ArgumentError] if +packed+ is outside the u64 range
    def unpack_u64(packed)
      raise ArgumentError, "packed out of u64 range: #{packed}" unless packed.between?(0, (1 << 64) - 1)

      [(packed >> 32) & U32_MASK, packed & U32_MASK]
    end
  end
end
