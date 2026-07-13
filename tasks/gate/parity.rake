# frozen_string_literal: true

# Coverage gate for the Ruby↔Rust differential parity harness
# (docs/parity.md): every CORE anchor must be either asserted by a
# scenario's `anchors:` list — so a scenario actually runs it through
# both frontends — or listed under Pending anchors, where no
# guest-expressible differential scenario exists and the behavior is
# pinned per-frontend. An anchor named only in a comment or skip
# message satisfies neither, so a scenario that silently degrades to
# comment-only fails the gate. Anchor resolvability itself is +rake
# anchors+' job.

require_relative "../support/report"

PARITY_ROOT = File.expand_path("../..", __dir__)
PARITY_MANIFEST = File.join(PARITY_ROOT, "docs/parity.md")
PARITY_TESTS = FileList[File.join(PARITY_ROOT, "test/parity/**/*.rb")]

# Anchors from the fenced block under a "## <heading>" section.
def parity_anchor_block(markdown, heading)
  section = markdown[/^## #{Regexp.escape(heading)}\n.*?```\n(.*?)```/m, 1]
  abort "gate:parity:coverage: docs/parity.md has no '#{heading}' block" unless section

  section.scan(/\b[BE]-\d+\b/).uniq
end

# Anchors a scenario actually runs, read only from `anchors: %w[...]`
# lists so a comment or skip-message citation never counts as coverage.
def parity_asserted_anchors(paths)
  paths.flat_map do |path|
    File.read(path).scan(/anchors:\s*%w\[([^\]]*)\]/).flat_map do |(list)|
      list.scan(/\b[BE]-\d+\b/)
    end
  end.uniq
end

namespace :gate do
  namespace :parity do
    desc "Check every CORE anchor in docs/parity.md is asserted or pending."
    task :coverage do
      markdown = File.read(PARITY_MANIFEST)
      manifest = parity_anchor_block(markdown, "CORE anchor manifest")
      pending = parity_anchor_block(markdown, "Pending anchors")
      asserted = parity_asserted_anchors(PARITY_TESTS)

      errors = (manifest - asserted - pending)
               .map { |anchor| "#{anchor} is neither asserted by a scenario nor listed as pending" }
      errors += (pending & asserted)
                .map { |anchor| "#{anchor} is asserted by a scenario — drop it from Pending anchors" }

      ok_summary = "#{manifest.size} CORE anchors " \
                   "(#{(manifest & asserted).size} asserted, #{(manifest & pending).size} pending)"
      puts KobakoReport.gate(name: "gate:parity:coverage", ok_summary: ok_summary,
                             violations: errors, noun: "coverage problem")
    end
  end
end
