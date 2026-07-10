# frozen_string_literal: true

# Inventory comparator backing +tasks/wire_symmetry.rake+
# (docs/wire-contract.md § Wire-Symmetric Peers): the wire-codable
# transport types and ext type codes of +lib/+ and +crates/kobako-codec+
# must match name-for-name; a one-sided entry needs a reasoned Accepted
# asymmetries entry, and the empty ledger is the target state.
module KobakoWireSymmetry
  module_function

  # A transport file's envelope participates in the wire when it defines
  # the value-object codec surface — +#encode+ or +.decode+; helper
  # methods such as the dispatcher's +encode_ok+ do not count.
  RUBY_CODEC_DEF = /^\s*def (?:self\.)?(?:encode|decode)\b/

  # The wire-codable class names in a +{ path => text }+ map of
  # +lib/kobako/transport/*.rb+ sources: the class carrying the codec
  # surface — the nearest +class+ above the first encode/decode — so a
  # preceding sibling class never takes the envelope's place.
  def ruby_types(sources)
    sources.values.filter_map do |text|
      codec_at = text =~ RUBY_CODEC_DEF
      text[0...codec_at].scan(/^\s*class (\w+)\b/).flatten.last if codec_at
    end.uniq.sort
  end

  # The type names carrying a +codec::Encode+ / +codec::Decode+ impl in
  # a +{ path => text }+ map of +crates/kobako-codec/src/transport/*.rs+
  # sources.
  def rust_types(sources)
    sources.values.flat_map do |text|
      text.scan(/^impl (?:codec::)?(?:Encode|Decode) for (\w+)/).flatten
    end.uniq.sort
  end

  # +{ name => code }+ from the Ruby ext-type registrations
  # (+EXT_SYMBOL = 0x00+ form).
  def ruby_ext_codes(text)
    text.scan(/EXT_(\w+)\s*=\s*(0x\h+)/).to_h
  end

  # +{ name => code }+ from the Rust ext-code constants
  # (+const EXT_SYMBOL: i8 = 0x00;+ form).
  def rust_ext_codes(text)
    text.scan(/const EXT_(\w+):\s*\w+\s*=\s*(0x\h+)/).to_h
  end

  # The entries in the fenced block under "### Accepted asymmetries";
  # +nil+ when the contract doc has no such block. Empty is the target
  # state.
  def accepted_asymmetries(markdown)
    block = markdown[/^### Accepted asymmetries\n.*?```\n?(.*?)```/m, 1]
    return nil unless block

    block.split.uniq
  end

  # Violation strings for every one-sided type or ext-code divergence
  # not carried by the Accepted asymmetries ledger, plus every ledger
  # entry the inventories no longer diverge on.
  def violations(ruby_types:, rust_types:, ruby_ext:, rust_ext:, accepted:)
    one_sided = (ruby_types - rust_types) + (rust_types - ruby_types) +
                (ruby_ext.keys - rust_ext.keys) + (rust_ext.keys - ruby_ext.keys)
    type_violations(ruby_types, rust_types, accepted) +
      ext_violations(ruby_ext, rust_ext, accepted) +
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

  def ext_violations(ruby_ext, rust_ext, accepted)
    one_sided_ext(ruby_ext, rust_ext, accepted) + mismatched_ext(ruby_ext, rust_ext)
  end

  def one_sided_ext(ruby_ext, rust_ext, accepted)
    ((ruby_ext.keys - rust_ext.keys) + (rust_ext.keys - ruby_ext.keys) - accepted)
      .map { |name| "ext type EXT_#{name} is registered on one side only" }
  end

  def mismatched_ext(ruby_ext, rust_ext)
    (ruby_ext.keys & rust_ext.keys)
      .reject { |name| ruby_ext[name] == rust_ext[name] }
      .map { |name| "ext type EXT_#{name} differs: #{ruby_ext[name]} in lib/, #{rust_ext[name]} in kobako-codec" }
  end
end
