# frozen_string_literal: true

# Append-only anchor checker backing +tasks/anchors.rake+. Enforces the
# N-8 invariant once the behavior spec is split across +docs/behavior/+:
# every +B-xx+ / +E-xx+ / +RX-xx+ / +JS-xx+ is defined exactly once, the
# sequence is contiguous up to the ceiling +SPEC.md+ states (gaps only where a
# retired tombstone declares one), and every reference resolves to a
# definition. +B+ / +RX+ / +JS+ anchors are defined by a Markdown heading, +E+
# anchors by an error-table row; +RX-xx+ (regexp.md) and +JS-xx+ (json.md) are
# topic-doc-local sequences with no SPEC ceiling, so each top is the highest
# anchor of that prefix defined.
module KobakoAnchors
  module_function

  # A reference token (+B-07+, +E-19+, +RX-03+, +JS-08+). The surrounding
  # boundaries keep it from binding inside a longer token such as a date
  # (+2026-06+) or an identifier, so prose, ranges, and tables all read the
  # same anchors a human would.
  REFERENCE = /(?<![A-Za-z0-9])(RX|JS|B|E)-(\d{1,3})(?![0-9])/

  # The numbers a prefix defines in +text+: +B+ / +RX+ from their
  # +## B-07 — +-style headings, +E+ from +| E-04 |+ table rows so an
  # inline +(E-04)+ reference is never mistaken for a definition.
  def definitions(text, prefix)
    pattern = prefix == "E" ? /^\|\s*E-(\d+)\s*\|/ : /^#+\s+#{prefix}-(\d+)\s+—/
    text.scan(pattern).flatten.map(&:to_i)
  end

  # The numbers +text+ declares retired for +prefix+ — the tombstone prose
  # "+<anchor> is a retired anchor …+" that licenses a hole in the sequence.
  def tombstones(text, prefix)
    text.scan(/\b#{prefix}-(\d+)\b[^\n]*?retired/i).flatten.map(&:to_i)
  end

  # Every anchor reference token in +text+, as +[prefix, number]+ pairs.
  def references(text)
    text.scan(REFERENCE).map { |prefix, number| [prefix, number.to_i] }
  end

  # Read +paths+ into a +{ relative_path => contents }+ map, with each key
  # made relative to +root+ so violation messages name a readable location.
  def read_sources(paths, root)
    paths.to_h { |path| [path.sub("#{root}/", ""), File.read(path)] }
  end

  # The SPEC-stated ceiling per prefix, read from the +### Refinement+
  # prose "+The current ceiling is B-50 / E-48+" so the checker and SPEC
  # cannot drift apart silently.
  def parse_ceilings(text)
    match = text.match(%r{current ceiling is B-(\d+)\s*/\s*E-(\d+)})
    return {} unless match

    { "B" => match[1].to_i, "E" => match[2].to_i }
  end

  # Audit a corpus and return the list of violation strings (empty = clean).
  # +def_sources+ maps each prefix to its authoritative +{ path => text }+
  # definition files; +ref_sources+ is every +{ path => text }+ scanned for
  # references; +ceilings+ carries the SPEC-stated top per prefix (+RX+ is
  # omitted — it derives its own).
  def audit(def_sources:, ref_sources:, ceilings:)
    defs = collect_definitions(def_sources)
    retired = collect_tombstones(def_sources)
    refs = collect_references(ref_sources)

    duplicate_violations(defs) +
      sequence_violations(defs, retired, ceilings) +
      ceiling_violations(defs, ceilings) +
      dangling_violations(defs, retired, refs)
  end

  # Map +{ prefix => { number => [defining paths] } }+ across all sources,
  # so a number appearing under more than one path is a duplicate.
  def collect_definitions(def_sources)
    def_sources.to_h do |prefix, files|
      sites = Hash.new { |hash, number| hash[number] = [] }
      files.each { |path, text| definitions(text, prefix).each { |number| sites[number] << path } }
      [prefix, sites]
    end
  end

  def collect_tombstones(def_sources)
    def_sources.to_h do |prefix, files|
      [prefix, files.values.flat_map { |text| tombstones(text, prefix) }.to_set]
    end
  end

  def collect_references(ref_sources)
    refs = Hash.new { |h, k| h[k] = [] }
    ref_sources.each do |path, text|
      references(text).each { |prefix, number| refs[prefix] << [number, path] }
    end
    refs
  end

  def duplicate_violations(defs)
    defs.flat_map do |prefix, sites|
      sites.select { |_n, paths| paths.size > 1 }
           .map { |n, paths| "duplicate #{prefix}-#{format("%02d", n)} defined in #{paths.join(", ")}" }
    end
  end

  def sequence_violations(defs, retired, ceilings)
    defs.flat_map do |prefix, sites|
      top = ceilings[prefix] || sites.keys.max || 0
      (1..top).filter_map do |n|
        next if sites.key?(n) || retired.fetch(prefix, Set.new).include?(n)

        "gap at #{prefix}-#{format("%02d", n)} — neither defined nor a retired tombstone"
      end
    end
  end

  def ceiling_violations(defs, ceilings)
    ceilings.filter_map do |prefix, stated|
      highest = defs.fetch(prefix, {}).keys.max
      next if highest.nil? || highest == stated

      "ceiling mismatch for #{prefix}: SPEC states #{stated} but highest defined is #{prefix}-#{highest}"
    end
  end

  def dangling_violations(defs, retired, refs)
    refs.flat_map do |prefix, citations|
      valid = defs.fetch(prefix, {}).keys.to_set | retired.fetch(prefix, Set.new)
      citations.filter_map do |number, path|
        next if valid.include?(number)

        "dangling #{prefix}-#{format("%02d", number)} referenced in #{path}"
      end.uniq
    end
  end
end
