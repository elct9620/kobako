# frozen_string_literal: true

# Payload-adapter replaceability gate (docs/wire-codec.md § How the Two
# Layers Relate): the core envelope must build with no payload adapter at
# all, and that build must carry no MessagePack dependency. Without this
# check "the adapter is replaceable" is a claim nothing verifies — the
# core layer could grow a `codec::` reference and only a third party
# assembling their own schema would find out.

CODEC_CRATE_DIR = File.expand_path("../../crates", __dir__)
CODEC_BARE = "--no-default-features"

namespace :gate do
  namespace :payload do
    desc "Check the core envelope builds with no payload adapter and no msgpack dependency."
    task :optional do
      violations = []
      Dir.chdir(CODEC_CRATE_DIR) do
        unless system("cargo check -p kobako-codec #{CODEC_BARE} --quiet", out: File::NULL)
          violations << "kobako-codec does not build #{CODEC_BARE} — " \
                        "the core envelope reaches into the payload adapter"
        end
        tree = `cargo tree -p kobako-codec #{CODEC_BARE} -e normal 2>/dev/null`
        pulled = tree.lines.drop(1).map(&:strip).reject(&:empty?)
        unless pulled.empty?
          violations << "the adapter-free build still pulls #{pulled.size} " \
                        "dependency/dependencies: #{pulled.join(", ")}"
        end
      end
      puts KobakoReport.gate(name: "gate:payload:optional",
                             ok_summary: "kobako-codec builds #{CODEC_BARE} with no dependencies",
                             violations: violations, noun: "violation")
    end
  end
end
