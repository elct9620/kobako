# frozen_string_literal: true

require_relative "anchors"

# Citation-profile reader backing +tasks/anchor_coverage.rake+
# (docs/anchor-coverage.md): every defined anchor's citing files under
# +test/+, the thin and most-cited ends of the profile, and the two gate
# rules — a zero-cited anchor must hold a Pending entry, and a Pending
# entry must stay uncited.
module KobakoAnchorCoverage
  module_function

  # Map +"<prefix>-<number>" => [citing files]+ for every anchor defined
  # in +def_sources+ (the +{ prefix => { path => text } }+ shape +rake
  # anchors+ audits). The counting unit is the distinct citing file —
  # mention counts inflate with witness-table size.
  def profile(def_sources:, test_sources:)
    citations = Hash.new { |hash, anchor| hash[anchor] = [] }
    test_sources.each do |path, text|
      KobakoAnchors.references(text).each { |prefix, number| citations[name(prefix, number)] << path }
    end

    defined_anchors(def_sources).to_h { |anchor| [anchor, citations[anchor].uniq.sort] }
  end

  # The anchors listed in the fenced block under "## Pending anchors";
  # a mention in the surrounding prose never counts. +nil+ when the
  # coverage doc has no such block.
  def pending_anchors(markdown)
    block = markdown[/^## Pending anchors\n.*?```\n(.*?)```/m, 1]
    return nil unless block

    KobakoAnchors.references(block).map { |prefix, number| name(prefix, number) }
  end

  # The gate: violation strings for a zero-cited anchor missing its
  # Pending entry and for a Pending entry a test now cites (stale). A
  # Pending entry naming an undefined anchor is +rake anchors+' dangling
  # check to report, not a stale entry.
  def violations(profile, pending)
    zero = profile.select { |_anchor, files| files.empty? }.keys
    stale = pending.select { |anchor| profile[anchor]&.any? }

    (zero - pending).map { |anchor| "#{anchor} has no citing test and no Pending anchors entry" } +
      stale.map { |anchor| "#{anchor} is cited by a test — drop it from Pending anchors" }
  end

  # The profile rows with at most one citing file — each is a candidate
  # for a new witness or a Pending entry.
  def thin(profile)
    profile.select { |_anchor, files| files.size <= 1 }.sort_by { |anchor, _files| sort_key(anchor) }
  end

  # The +limit+ most-cited profile rows — candidates for duplicate
  # coverage review.
  def top(profile, limit: 5)
    profile.sort_by { |anchor, files| [-files.size, sort_key(anchor)] }.first(limit)
  end

  # The printable report: the thin end (a Pending entry reads +pending+,
  # a bare zero reads +UNCITED+) followed by the most-cited end.
  def report_lines(profile, pending)
    thin_lines = thin(profile).map do |anchor, files|
      detail = files.first || (pending.include?(anchor) ? "pending" : "UNCITED")
      format("  %<anchor>-6s %<detail>s", anchor: anchor, detail: detail)
    end
    top_lines = top(profile).map do |anchor, files|
      format("  %<anchor>-6s %<count>d files", anchor: anchor, count: files.size)
    end
    ["thin (at most one citing file):", *thin_lines, "most cited:", *top_lines]
  end

  def name(prefix, number)
    format("%<prefix>s-%<number>02d", prefix: prefix, number: number)
  end

  def defined_anchors(def_sources)
    def_sources.flat_map do |prefix, files|
      files.values.flat_map { |text| KobakoAnchors.definitions(text, prefix) }
                  .map { |number| name(prefix, number) }
    end
  end

  def sort_key(anchor)
    prefix, number = anchor.split("-")
    [prefix, number.to_i]
  end
end
