# frozen_string_literal: true

# Coverage gate for the Ruby↔Rust differential parity harness
# (docs/parity.md): every CORE anchor in the manifest must be cited by
# at least one scenario or pending entry under test/parity/, so a new
# host-observable behavior cannot land on one frontend without the
# other side's check appearing here. Anchor resolvability itself is
# +rake anchors+' job.

PARITY_ROOT = File.expand_path("..", __dir__)
PARITY_MANIFEST = File.join(PARITY_ROOT, "docs/parity.md")
PARITY_TESTS = FileList[File.join(PARITY_ROOT, "test/parity/**/*.rb")]

# The manifest is the fenced block under "## CORE anchor manifest".
def parity_core_anchors(markdown)
  section = markdown[/^## CORE anchor manifest\n.*?```\n(.*?)```/m, 1]
  abort "parity: docs/parity.md has no CORE anchor manifest block" unless section

  section.scan(/\b[BE]-\d+\b/).uniq
end

namespace :parity do
  desc "Check every CORE anchor in docs/parity.md is cited under test/parity/."
  task :coverage do
    manifest = parity_core_anchors(File.read(PARITY_MANIFEST))
    cited = PARITY_TESTS.flat_map { |path| File.read(path).scan(/\b[BE]-\d+\b/) }.uniq
    uncovered = manifest - cited

    if uncovered.empty?
      puts "parity: OK — #{manifest.size} CORE anchors all cited under test/parity/"
    else
      uncovered.each { |anchor| warn "  parity: #{anchor} has no scenario or pending entry" }
      abort "parity: #{uncovered.size} CORE anchor(s) uncovered"
    end
  end
end
