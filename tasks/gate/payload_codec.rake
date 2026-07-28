# frozen_string_literal: true

# Payload-codec replaceability gate (docs/wire-codec.md § How the Two
# Layers Relate): the tiers that only route messages must build with no
# payload codec at all, and those builds must carry no MessagePack
# dependency. Without this check "the codec is replaceable" is a claim
# nothing verifies — a tier could grow a `codec::` reference and only a
# third party assembling their own schema would find out.
#
# `kobako-transport` is the fixed tier every assembly composes against;
# `kobako-core` is the guest ABI contract every third-party guest builds
# on; `kobako-mruby` is the harness whose `MrbGuest::Codec` the shell
# names — so a guest that speaks another schema reaches all three without
# MessagePack in its graph. The Rust host SDK is the same claim from the
# other side, but it legitimately carries a whole wasm engine — so it is
# held to the narrower property that matters: no payload codec anywhere
# in its codec-free graph.
#
# The tiers are probed on their default build — that is the graph a third
# party gets for naming one, and Cargo's default set is public surface a
# release can add to but never take from.
#
# Every probe runs +--all-targets+: a tier whose library builds codec-free
# while its tests do not has moved the codec out of the shipped graph
# without moving it out of the code, and only a third party would find
# out. Reaching a payload codec from a +[dev-dependencies]+ entry stays
# fine — +cargo tree -e normal+ does not see one, and neither does anyone
# installing the crate.

# Each tier whose default build must stand without a payload codec, with
# the workspace its manifest lives in.
CODEC_FREE_TIERS = {
  "kobako-transport" => File.expand_path("../../crates", __dir__),
  "kobako-core" => File.expand_path("../../wasm", __dir__),
  "kobako-mruby" => File.expand_path("../../wasm", __dir__)
}.freeze
# Crates a codec-free build may still resolve to: the tiers themselves
# plus the mruby wrapper, which is the interpreter rather than a schema.
CODEC_FREE_ALLOWED = (CODEC_FREE_TIERS.keys + %w[beni beni-sys]).freeze

# Report why +crate+ is not codec-free, or +nil+ when it is. A crate is
# codec-free when its default build resolves to nothing but the
# routing-only tiers themselves.
def codec_free_violation(crate, dir)
  Dir.chdir(dir) do
    unless system("cargo check -p #{crate} --all-targets --quiet", out: File::NULL)
      next "#{crate}'s default build fails — it reaches into the payload codec"
    end

    codec_free_tree_violation(crate)
  end
end

# What the default build of +crate+ resolves to beyond the routing-only
# tiers, as a violation string, or +nil+ when it resolves to nothing else.
# Runs inside the crate's workspace directory.
def codec_free_tree_violation(crate)
  tree = `cargo tree -p #{crate} -e normal 2>/dev/null`
  pulled = tree.lines.drop(1).filter_map { |line| line[/[a-z0-9-]+(?= v[0-9])/] }
  external = pulled.reject { |dep| CODEC_FREE_ALLOWED.include?(dep) }
  return if external.empty?

  "#{crate}'s codec-free build still pulls #{external.size} " \
    "dependency/dependencies: #{external.join(", ")}"
end

# The crates a codec-free graph must not contain at all: the payload
# codecs themselves and the MessagePack library beneath them. Named
# rather than derived so adding a codec crate is a deliberate edit here.
CODEC_CRATES = %w[kobako-codec rmp rmp-serde msgpack].freeze
# Tiers that may depend on anything except a payload codec — the Rust
# host SDK, which drives a wasm engine and so can never resolve to
# nothing, yet must still reach no schema when built codec-free. It is
# the frontend an embedder names directly, so unlike the tiers above it
# defaults to a codec and the claim is probed with that deselected.
CODEC_DESELECTED = "--no-default-features"
CODEC_ABSENT_TIERS = { "kobako" => File.expand_path("../../crates", __dir__) }.freeze

# Report why +crate+'s codec-free build still reaches a payload codec, or
# +nil+ when it reaches none.
def codec_absent_violation(crate, dir)
  Dir.chdir(dir) do
    unless system("cargo check -p #{crate} #{CODEC_DESELECTED} --all-targets --quiet",
                  out: File::NULL)
      next "#{crate} does not build #{CODEC_DESELECTED} — it reaches into the payload codec"
    end

    codec_reached_violation(crate)
  end
end

# Which payload codecs +crate+'s codec-free graph still reaches, as a
# violation string, or +nil+ when it reaches none. Runs inside the crate's
# workspace directory.
def codec_reached_violation(crate)
  tree = `cargo tree -p #{crate} #{CODEC_DESELECTED} -e normal 2>/dev/null`
  pulled = tree.lines.drop(1).filter_map { |line| line[/[a-z0-9-]+(?= v[0-9])/] }
  found = pulled.uniq & CODEC_CRATES
  return if found.empty?

  "#{crate}'s codec-free build still reaches a payload codec: #{found.join(", ")}"
end

namespace :gate do
  namespace :payload do
    desc "Check the routing-only tiers build with no payload codec and no msgpack dependency."
    task :optional do
      violations = CODEC_FREE_TIERS.filter_map { |crate, dir| codec_free_violation(crate, dir) }
      violations += CODEC_ABSENT_TIERS.filter_map { |crate, dir| codec_absent_violation(crate, dir) }
      total = CODEC_FREE_TIERS.size + CODEC_ABSENT_TIERS.size
      puts KobakoReport.gate(name: "gate:payload:optional",
                             ok_summary: "#{total} tiers reach no payload codec — " \
                                         "#{CODEC_FREE_TIERS.size} on their default build, " \
                                         "#{CODEC_ABSENT_TIERS.size} with the codec deselected",
                             violations: violations, noun: "violation")
    end
  end
end
