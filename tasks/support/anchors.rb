# frozen_string_literal: true

# Append-only anchor checker backing +tasks/gate/anchors.rake+. Enforces the
# N-8 invariant for the families +SPEC.md+ defines for itself — +F-xx+
# features, +J-xx+ journeys, +N-x+ naming principles: each is defined exactly
# once, its sequence is contiguous (gaps only where a retired tombstone
# declares one), and every reference resolves to a definition.
module KobakoAnchors
  module_function

  # A reference token (+F-10+, +J-06+, +N-8+). The surrounding boundaries keep
  # it from binding inside a longer token — a date (+2026-06+), an identifier,
  # or a three-digit sumi scenario id (+J-010+) — so prose, ranges, and tables
  # read the same anchors a human would.
  REFERENCE = /(?<![A-Za-z0-9])(F|J|N)-(\d{1,2})(?![0-9])/

  # The prefixes whose definition site is a +| F-01 |+-style table row — +J+
  # defines by a +#### J-01 — +-style heading — so an inline +(F-01)+
  # reference is never mistaken for a definition.
  TABLE_DEFINED = %w[F N].freeze

  # The numbers a prefix defines in +text+, read from its definition shape.
  def definitions(text, prefix)
    pattern = if TABLE_DEFINED.include?(prefix)
                /^\|\s*#{prefix}-(\d+)\s*\|/
              else
                /^#+\s+#{prefix}-(\d+)\s+—/
              end
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

  # Audit a corpus and return the list of violation strings (empty = clean).
  # +def_sources+ maps each prefix to its authoritative +{ path => text }+
  # definition files; +ref_sources+ is every +{ path => text }+ scanned for
  # references.
  def audit(def_sources:, ref_sources:)
    defs = collect_definitions(def_sources)
    retired = collect_tombstones(def_sources)
    refs = collect_references(ref_sources)

    duplicate_violations(defs) +
      sequence_violations(defs, retired) +
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

  def sequence_violations(defs, retired)
    defs.flat_map do |prefix, sites|
      top = sites.keys.max || 0
      (1..top).filter_map do |n|
        next if sites.key?(n) || retired.fetch(prefix, Set.new).include?(n)

        "gap at #{prefix}-#{format("%02d", n)} — neither defined nor a retired tombstone"
      end
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
