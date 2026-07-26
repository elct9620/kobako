# frozen_string_literal: true

# Payload-codec replaceability gate (docs/wire-codec.md § How the Two
# Layers Relate): the tiers that only route messages must build with no
# payload codec at all, and those builds must carry no MessagePack
# dependency. Without this check "the codec is replaceable" is a claim
# nothing verifies — either tier could grow a `codec::` reference and only
# a third party assembling their own schema would find out.
#
# `kobako-codec` is the wire tier itself; `kobako-core` is the guest ABI
# contract every third-party guest builds on; `kobako-mruby` is the
# harness whose `MrbGuest::Codec` the shell names — so a guest that
# speaks another schema reaches all three without MessagePack in its graph.

CODEC_BARE = "--no-default-features"
# Each tier that must stand without a payload codec, with the workspace
# its manifest lives in.
CODEC_FREE_TIERS = {
  "kobako-codec" => File.expand_path("../../crates", __dir__),
  "kobako-core" => File.expand_path("../../wasm", __dir__),
  "kobako-mruby" => File.expand_path("../../wasm", __dir__)
}.freeze
# Crates a codec-free build may still resolve to: the tiers themselves
# plus the mruby wrapper, which is the interpreter rather than a schema.
CODEC_FREE_ALLOWED = (CODEC_FREE_TIERS.keys + %w[beni beni-sys]).freeze

# Report why +crate+ is not codec-free, or +nil+ when it is. A crate is
# codec-free when it builds with no codec selected and that build
# resolves to nothing but the routing-only tiers themselves.
def codec_free_violation(crate, dir)
  Dir.chdir(dir) do
    unless system("cargo check -p #{crate} #{CODEC_BARE} --quiet", out: File::NULL)
      next "#{crate} does not build #{CODEC_BARE} — it reaches into the payload codec"
    end

    codec_free_tree_violation(crate)
  end
end

# What a codec-free build of +crate+ resolves to beyond the routing-only
# tiers, as a violation string, or +nil+ when it resolves to nothing else.
# Runs inside the crate's workspace directory.
def codec_free_tree_violation(crate)
  tree = `cargo tree -p #{crate} #{CODEC_BARE} -e normal 2>/dev/null`
  pulled = tree.lines.drop(1).filter_map { |line| line[/[a-z0-9-]+(?= v[0-9])/] }
  external = pulled.reject { |dep| CODEC_FREE_ALLOWED.include?(dep) }
  return if external.empty?

  "#{crate}'s codec-free build still pulls #{external.size} " \
    "dependency/dependencies: #{external.join(", ")}"
end

namespace :gate do
  namespace :payload do
    desc "Check the routing-only tiers build with no payload codec and no msgpack dependency."
    task :optional do
      violations = CODEC_FREE_TIERS.filter_map { |crate, dir| codec_free_violation(crate, dir) }
      puts KobakoReport.gate(name: "gate:payload:optional",
                             ok_summary: "#{CODEC_FREE_TIERS.size} routing-only tiers build " \
                                         "#{CODEC_BARE} with no codec dependency",
                             violations: violations, noun: "violation")
    end
  end
end
