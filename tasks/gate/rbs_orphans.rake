# frozen_string_literal: true

# +rake gate:rbs:orphans+ — gate that every declaration under +sig/+ names
# something the implementation defines.
#
# Steep reads Ruby against RBS and never the reverse, and +rbs validate+
# holds the signatures only to themselves, so a declaration a deletion
# left behind rides a green suite indefinitely. This is the direction
# nothing else covers. Resolution needs the library loaded, which is why
# the gate reaches for the built extension rather than reading text. Its
# readers ride the tooling suite (+test/tasks/test_rbs_orphans.rb+).

require_relative "../support/rbs_orphans"
require_relative "../support/report"

namespace :gate do
  namespace :rbs do
    desc "Check every sig/ declaration names something the implementation defines."
    task :orphans do
      $LOAD_PATH.unshift(File.expand_path("../../lib", __dir__))
      require "kobako"

      sources = Dir["sig/kobako/**/*.rbs"].to_h { |path| [path, File.read(path)] }
      declarations = KobakoRbsOrphans.declarations(sources)

      violations = KobakoRbsOrphans.orphans(declarations).map do |orphan|
        "  #{KobakoRbsOrphans.spell(orphan)} — declared in #{orphan.last}, defined nowhere"
      end

      puts KobakoReport.gate(name: "gate:rbs:orphans",
                             ok_summary: "#{declarations.size} declarations name a live definition",
                             violations: violations, noun: "orphan")
    end
  end
end
