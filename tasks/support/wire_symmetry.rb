# frozen_string_literal: true

# Inventory comparator backing +tasks/gate/wire_symmetry.rake+
# (docs/wire-contract.md § Wire-Symmetric Peers): the payload type names
# of +lib/+ and +crates/kobako-codec+ must match one another; a one-sided
# name needs a reasoned Accepted asymmetries entry, and the empty ledger
# is the target state.
module KobakoWireSymmetry
  module_function

  # A payload class participates in the wire when it defines the
  # value-object codec surface — +#encode+ or +.decode+; helper methods
  # such as the dispatcher's +encode_ok+ do not count.
  RUBY_CODEC_DEF = /^\s*def (?:self\.)?(?:encode|decode)\b/

  # The wire-codable class names in a +{ path => text }+ map of the Ruby
  # transport and payload tiers: each encode/decode def is attributed to
  # the nearest +class+ above it, so a preceding sibling class never takes
  # a codec-bearing class's place and a second one in the same file never
  # vanishes behind the first.
  def ruby_types(sources)
    sources.values.flat_map { |text| codec_classes(text) }.uniq.sort
  end

  def codec_classes(text)
    text.enum_for(:scan, RUBY_CODEC_DEF).filter_map do
      text[0...Regexp.last_match.begin(0)].scan(/^\s*class (\w+)\b/).flatten.last
    end
  end

  # The type names carrying a +codec::Encode+ / +codec::Decode+ impl in a
  # +{ path => text }+ map of the Rust peer's transport and payload tiers.
  def rust_types(sources)
    sources.values.flat_map do |text|
      text.scan(/^impl (?:codec::)?(?:Encode|Decode) for (\w+)/).flatten
    end.uniq.sort
  end

  # The entries in the fenced block under "### Accepted asymmetries";
  # +nil+ when the contract doc has no such block. Empty is the target
  # state.
  def accepted_asymmetries(markdown)
    block = markdown[/^### Accepted asymmetries\n.*?```\n?(.*?)```/m, 1]
    return nil unless block

    block.split.uniq
  end

  # Violation strings for every one-sided type not carried by the Accepted
  # asymmetries ledger, plus every ledger entry the inventories no longer
  # diverge on.
  def violations(ruby_types:, rust_types:, accepted:)
    one_sided = (ruby_types - rust_types) + (rust_types - ruby_types)
    type_violations(ruby_types, rust_types, accepted) +
      stale_accepted(accepted, one_sided)
  end

  # The ledger's staleness half, mirroring the Pending-anchors rule: an
  # accepted entry with no current divergence is dead weight to shed.
  def stale_accepted(accepted, one_sided)
    (accepted - one_sided)
      .map { |name| "accepted asymmetry #{name} no longer diverges — drop it from the ledger" }
  end

  def type_violations(ruby_types, rust_types, accepted)
    (ruby_types - rust_types - accepted)
      .map { |name| "#{name} is wire-codable only in lib/ — missing its kobako-codec peer" } +
      (rust_types - ruby_types - accepted)
      .map { |name| "#{name} is wire-codable only in kobako-codec — missing its lib/ peer" }
  end
end
