# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the outcome (#eval return) value path through real mruby:
# embedded-NUL fidelity, the 128-level structural depth guard, Float /
# Integer bit-fidelity, native Array / Hash round-trips, and the
# +try_codec_value+ raise-on-unrepresentable contract (E-06). The transport
# (dispatch-arg) counterpart lives in test_dispatch_args.rb.
class TestE2EOutcomeValues < Minitest::Test
  include E2eGuestHelper

  # The wire str type is UTF-8 text (docs/wire-codec.md § Type Mapping #5)
  # and an embedded NUL is a valid UTF-8 codepoint, so a String / Symbol /
  # Hash key carrying one must round-trip as an ordinary result. The guest
  # result encoder read mruby strings as C strings, which truncate at and
  # raise on NUL; on the outcome-encode path that raise had no protect frame
  # and hard-trapped the whole eval. Reading by length keeps the NUL bytes
  # and the value crosses the boundary intact. The three shapes exercise the
  # distinct encoder branches that all funnel through the same string read.
  def test_embedded_nul_round_trips_through_the_result_encoder
    {
      '"a\x00b"' => "a\x00b",
      '"a\x00b".to_sym' => :"a\x00b",
      '{ "k\x00" => 1 }' => { "k\x00" => 1 }
    }.each do |code, expected|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

      assert_equal expected, sandbox.eval(code),
                   "a returned value carrying an embedded NUL (#{code}) must round-trip " \
                   "intact through the result encoder, not hard-trap the eval"
    end
  end

  # The same length-based string read governs the Panic envelope message, a
  # separate call site from the result path: a raised exception whose message
  # holds a NUL must surface as a clean, rescuable SandboxError rather than an
  # unrescuable hard trap.
  def test_embedded_nul_in_raised_message_is_a_clean_sandbox_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) { sandbox.eval('raise "a\x00b"') }

    assert_equal "RuntimeError", err.klass,
                 "a NUL in a raised message must reach the host as a clean SandboxError, not a trap"
    assert_match(/a\x00b/, err.message,
                 "the NUL-bearing message must survive the length-based read intact")
  end

  # docs/wire-codec.md § Structural Nesting Depth: the guest encoder caps its
  # recursive walk at 128 levels — the MessagePack limit the host decoder
  # already enforces. A return value that nests deeper, or that holds a
  # reference cycle (unbounded depth), has no wire representation and must
  # surface as a clean, rescuable SandboxError (E-06) rather than overflowing
  # the wasm stack into an unrescuable hard trap. A direct Array cycle, a Hash
  # self-cycle, and a structure far past the cap each exercise the guard.
  def test_over_depth_or_cyclic_result_is_a_clean_sandbox_error
    ["a = []; a << a; a", "h = {}; h[:self] = h; h", "a = 0; 5000.times { a = [a] }; a"].each do |code|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
      assert_raises(Kobako::SandboxError,
                    "an over-deep or cyclic result (#{code}) must fail cleanly, not hard-trap") do
        sandbox.eval(code)
      end
    end
  end

  # A structure nested within the cap round-trips unaffected — the guard
  # rejects only what would otherwise overflow, not ordinary nested data.
  # Unwrapping every level pins that all 100 levels and the innermost value
  # survive, so a silent truncation to a shallower array would still fail.
  def test_nesting_within_the_cap_round_trips
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval("a = 0; 100.times { a = [a] }; a")

    depth = 0
    node = result
    while node.is_a?(Array)
      depth += 1
      node = node.first
    end

    assert_equal 100, depth, "all 100 nesting levels must round-trip, not be truncated"
    assert_equal 0, node, "the innermost value must survive the round-trip intact"
  end

  # H-1 regression: a Float returned from the guest must reach the host
  # bit-identical, not via `Float#to_s` + Rust `parse` (which used the
  # mruby %.16g formatter and could drop the last ULP on edge values).
  # `0.1 + 0.2` is the canonical witness: the IEEE-754 result is
  # `0.30000000000000004` and any narrower text formatting would lose
  # the trailing 4. Asserting bit equality via `==` is sufficient
  # because the codec encodes Float as msgpack `float 64`.
  def test_outcome_float_precision_round_trips_full_f64
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    result = sandbox.eval("0.1 + 0.2")
    assert_equal 0.30000000000000004, result,
                 "H-1: Float outcome must round-trip the full 64-bit payload"
  end

  # H-2 regression: an Integer must round-trip via the direct unbox
  # path — a text-coercion round-trip would silently fall back to 0 on
  # parse failure. mruby's MRB_INT32 word-box reserves a
  # tag bit on wasm32, so the addressable Fixnum range is narrower than
  # i32; use 2^28 ± 1 as a representative magnitude that exercises the
  # signed 32-bit return path of `kobako_fixnum_value` without leaving
  # the Fixnum-tagged range.
  def test_outcome_integer_round_trips_via_direct_unbox
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    assert_equal 268_435_457, sandbox.eval("268_435_457")
    assert_equal(-268_435_457, sandbox.eval("-268_435_457"))
  end

  # outcome path: +try_codec_value+ raises on a type outside the 12-entry
  # wire type set rather than handing the host a misleading String (E-06;
  # SPEC.md pins "no implicit inspect / to_h / to_s conversion"). The
  # transport (dispatch-arg) path rejects the same way (E-55) — its pin
  # lives in test_dispatch_args.rb.
  UNREPRESENTABLE_OUTCOME_SCRIPT = "Object.new"

  def test_outcome_unrepresentable_value_raises_sandbox_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval(UNREPRESENTABLE_OUTCOME_SCRIPT)
    end

    assert_match(/not a supported sandbox value type/, err.message,
                 "E-06: an #eval return value of an unsupported type must take " \
                 "the Panic path as Kobako::SandboxError, never an implicit inspect String")
  end

  # ── Native Array / Hash round-trips (SPEC.md Type Mapping #7-#8) ──────
  #
  # The 12-entry Type Mapping (SPEC.md → Wire Codec → Type Mapping) maps
  # msgpack array → mruby Array and msgpack map → mruby Hash. Both
  # directions must travel by value with element-level fidelity (SPEC.md
  # B-13: "Collections (Array, Hash) whose elements are all
  # wire-representable are transmitted in full by value").

  # Outcome path: a script whose last expression is an mruby Array must
  # serialize as +Value::Array+ on the wire, not as the +inspect+
  # string. Mixed-element fidelity (Integer + String + Symbol) is part
  # of the contract.
  def test_outcome_array_returns_native_array
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval('[1, "a", :b]')

    assert_equal [1, "a", :b], result,
                 "outcome path: mruby Array must arrive as a Ruby Array with " \
                 "preserved element types (SPEC.md Type Mapping #7)"
  end

  # Outcome path: an mruby Hash must serialize as +Value::Map+ and
  # arrive as a Ruby Hash. Symbol-vs-String key distinction is part of
  # the wire contract — SPEC.md Ext Types pins that
  # +"a"+ and +:a+ are not wire-equivalent.
  def test_outcome_hash_returns_native_hash
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval('{a: 1, "b" => 2}')

    assert_equal({ a: 1, "b" => 2 }, result,
                 "outcome path: mruby Hash must arrive as a Ruby Hash preserving " \
                 "the Symbol-vs-String key distinction (SPEC.md Type Mapping #8 + ext 0x00)")
  end

  # Empty collection round-trips. These two tests pin the canonical
  # wire encoding end-to-end — an empty Hash is +Value::Map(vec![])+,
  # never a +"{}"+ string sentinel — so any converter regression that
  # re-introduces a sentinel string surfaces immediately.
  def test_outcome_empty_array_round_trips
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_equal [], sandbox.eval("[]"),
                 "outcome path: empty Array must arrive as `[]`, not the inspect string"
  end

  def test_outcome_empty_hash_round_trips
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_equal({}, sandbox.eval("{}"),
                 "outcome path: empty Hash must arrive as `{}`, not the legacy `\"{}\"` sentinel")
  end
end
