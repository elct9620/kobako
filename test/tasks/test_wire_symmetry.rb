# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/wire_symmetry"

# Unit coverage for the wire-symmetric peer comparator
# (docs/wire-contract.md § Wire-Symmetric Peers): type extraction on
# each side counts only the value-object codec surface, the Accepted
# asymmetries ledger parses from its fenced block alone, and every
# one-sided or code-mismatched divergence is a violation.
class KobakoWireSymmetryTest < Minitest::Test
  Symmetry = KobakoWireSymmetry

  def test_ruby_types_count_only_envelope_codec_classes
    sources = {
      "yield.rb" => "class Yield < Data.define(:tag)\n  def encode\n  end\nend\n",
      "run.rb" => "class Run\n  def encode(handler)\n  end\nend\n",
      "dispatcher.rb" => "class Dispatcher\n  def encode_ok(value)\n  end\nend\n"
    }

    assert_equal %w[Run Yield], Symmetry.ruby_types(sources),
                 "a dispatcher helper named encode_ok must not count as a wire-codable envelope"
  end

  # The envelope is the class that carries the codec surface — a
  # preceding sibling class in the same file must never take its place.
  def test_ruby_types_name_the_class_carrying_the_codec_surface
    sources = { "run.rb" => <<~RB }
      class EntrypointError < Error
      end

      class Run
        def encode(handler)
        end
      end
    RB

    assert_equal %w[Run], Symmetry.ruby_types(sources),
                 "the inventory must name the encode/decode-bearing class, not the file's first class"
  end

  # A second codec-bearing class in the same file must not vanish
  # behind the first: a Ruby-only envelope added there would otherwise
  # pass the gate without its kobako-codec peer.
  def test_ruby_types_inventory_every_codec_class_in_one_file
    sources = { "pair.rb" => <<~RB }
      class Ping
        def encode = nil
      end

      class Pong
        def self.decode(bytes) = nil
      end
    RB

    assert_equal %w[Ping Pong], Symmetry.ruby_types(sources),
                 "a file holding two codec-bearing classes through ruby_types must inventory both"
  end

  def test_rust_types_read_both_bare_and_qualified_impls
    sources = {
      "block.rs" => "impl Encode for Yield {\n}\nimpl Decode for Yield {\n}\n",
      "request.rs" => "impl codec::Encode for Request {\n}\n"
    }

    assert_equal %w[Request Yield], Symmetry.rust_types(sources),
                 "bare and codec-qualified impls through rust_types must inventory each type once, sorted"
  end

  def test_ext_codes_extract_name_to_code_maps
    ruby = "EXT_SYMBOL = 0x00\nEXT_HANDLE = 0x01\n"
    rust = "const EXT_SYMBOL: i8 = 0x00;\nconst EXT_HANDLE: i8 = 0x01;\n"

    assert_equal Symmetry.ruby_ext_codes(ruby), Symmetry.rust_ext_codes(rust),
                 "matching EXT_ registrations through both ext-code readers must yield one name => code map"
  end

  def test_accepted_asymmetries_parse_the_fenced_block_even_when_empty
    markdown = "### Accepted asymmetries\n\n```\n```\n"

    assert_empty Symmetry.accepted_asymmetries(markdown),
                 "an empty fenced block through accepted_asymmetries must read as no accepted entries"
  end

  def test_accepted_asymmetries_is_nil_without_the_block
    assert_nil Symmetry.accepted_asymmetries("# No ledger here\n"),
               "a contract doc without the ledger block through accepted_asymmetries must read as nil"
  end

  def test_one_sided_type_without_ledger_entry_is_a_violation
    violations = Symmetry.violations(
      ruby_types: %w[Request Yield], rust_types: %w[Request],
      ruby_ext: {}, rust_ext: {}, accepted: []
    )

    assert_equal ["Yield is wire-codable only in lib/ — missing its kobako-codec peer"], violations,
                 "a type on one side with no ledger entry through violations must surface as a missing peer"
  end

  def test_ledger_entry_silences_a_one_sided_type
    violations = Symmetry.violations(
      ruby_types: %w[Request], rust_types: %w[Request Probe],
      ruby_ext: {}, rust_ext: {}, accepted: %w[Probe]
    )

    assert_empty violations,
                 "a one-sided type carried by the ledger through violations must not surface"
  end

  # The staleness half of the ledger gate, mirroring the Pending-anchors
  # rule: an entry the inventories no longer diverge on is dead weight
  # the ledger must shed.
  def test_ledger_entry_with_no_current_divergence_is_a_violation
    violations = Symmetry.violations(
      ruby_types: %w[Request], rust_types: %w[Request],
      ruby_ext: { "HANDLE" => "0x01" }, rust_ext: { "HANDLE" => "0x01" }, accepted: %w[Probe]
    )

    assert_equal ["accepted asymmetry Probe no longer diverges — drop it from the ledger"], violations,
                 "a ledger entry with no current divergence through violations must surface as stale"
  end

  def test_ext_code_value_mismatch_is_a_violation_even_when_both_sides_name_it
    violations = Symmetry.violations(
      ruby_types: [], rust_types: [],
      ruby_ext: { "HANDLE" => "0x01" }, rust_ext: { "HANDLE" => "0x02" }, accepted: []
    )

    assert_equal ["ext type EXT_HANDLE differs: 0x01 in lib/, 0x02 in kobako-codec"], violations,
                 "an ext code differing across sides through violations must surface even when both name it"
  end
end
