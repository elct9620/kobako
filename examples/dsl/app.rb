#!/usr/bin/env ruby
# frozen_string_literal: true

# Host-owned DSL demo: the guest writes an idiomatic, receiver-less builder
# (`header ...`, `body do ... end`) whose every dialect lives on the HOST. No
# builder logic is reimplemented in the guest — one small generic wrapper
# forwards each call onto a Capability Handle, so the host's card/section/image
# classes stay the single source of truth.
#
# The lesson this example encodes
# -------------------------------
# A host-object DSL needs no new kobako feature — it composes three existing
# behaviours:
#
#   * a Service method returns a stateful object, which crosses as a
#     Capability Handle (B-14);
#   * the guest calls methods on that Handle, and the returned child Handle
#     chains to arbitrary depth (B-17);
#   * the reflection denial is scoped to guest→host dispatch, so a guest-LOCAL
#     `instance_eval` on a plain wrapper object is permitted (B-42 / B-44).
#
# `Studio.card` returns a card Handle; `body` returns a section Handle; the
# generic `Build` wrapper descends into each returned child with
# `instance_eval`, so the guest writes a receiver-less DSL while the vocabulary
# at each level is whatever the host object at that level defines — a method the
# host does not define is refused host-side (B-42), so the wrapper can never
# widen the reachable surface.
#
# The one wrapper rule that matters
# ---------------------------------
# The wrapper wraps a returned Handle only when a block is given (a container to
# descend into); a block-less call returns the RAW Handle. That is deliberate:
# a value object like an image is fetched block-lessly (`logo = Studio.image`)
# and then passed as an argument (`image logo`), where it crosses as a Handle
# and is restored to the real host object (B-16). A guest wrapper object has no
# wire representation, so passing one as an argument is refused (E-55) — keeping
# leaves raw is what lets them travel.
#
# Usage:
#   ruby examples/dsl/app.rb
#
# From a clone of the kobako repository, prefix with `bundle exec` so the
# local checkout is used instead of the released gem.

require "bundler/inline"

gemfile do
  source "https://rubygems.org"
  gem "kobako", "~> 0.16.0"
end

require "kobako"
require "json"

# Example types are nested under Dsl so the file carries a single top-level
# constant and reads top-down.
module Dsl
  # The guest idiom, preloaded as the Extension's source: ONE generic wrapper.
  # It defines no dialect — `method_missing` forwards a dynamic method name
  # onto the wrapped Handle, and, when a block is given, descends into the
  # returned child Handle via `instance_eval`. A block-less call returns the
  # raw result so a leaf Handle can be passed on as an argument.
  BUILD_SOURCE = <<~MRUBY
    class Build
      def initialize(handle) = (@handle = handle)

      def method_missing(name, *args, &blk)
        result = @handle.public_send(name, *args)
        return result unless blk

        (result.is_a?(Kobako::Handle) ? Build.new(result) : result).instance_eval(&blk)
        self
      end

      def respond_to_missing?(_name, _include_private = false) = true

      def handle = @handle
    end
  MRUBY

  # The guest program: a receiver-less card DSL. Each `do ... end` descends into
  # a host dialect; `logo` is fetched block-lessly and passed as an argument.
  CARD_SCRIPT = <<~MRUBY
    logo = Studio.image(url: "https://cdn.example/logo.png")

    card = Build.new(Studio.card)
    card.instance_eval do
      header "Welcome aboard"
      body do
        text "Thanks for joining."
        image logo
        group do
          text "Your trial ends in 14 days."
          button "Upgrade", "action://upgrade"
        end
      end
      footer do
        text "Sent by the host, rendered nowhere near it."
      end
    end
    card.handle.result
  MRUBY

  # A method no host dialect defines, forwarded through the wrapper. The host
  # refuses it (B-42), so the DSL's vocabulary can never exceed the host's.
  UNKNOWN_VERB_SCRIPT = <<~MRUBY
    Build.new(Studio.card).instance_eval { marquee "not a card method" }
  MRUBY

  # ---- Host dialects. Each builder method returns the child it created, so
  # the guest can descend into it; the tree is assembled entirely host-side. --

  # A leaf value object. It is fetched block-lessly by the guest and passed as
  # an argument, so it crosses as a Handle and is restored here (B-16).
  class Image
    def initialize(url) = (@url = url)
    def to_h = { "type" => "image", "url" => @url }
  end

  # A section dialect: text / image / button leaves, plus `group` for nesting.
  class Section
    def initialize(kind) = (@node = { "type" => kind, "items" => [] })

    def text(value)
      @node["items"] << { "type" => "text", "text" => value }
      nil
    end

    def button(label, action)
      @node["items"] << { "type" => "button", "label" => label, "action" => action }
      nil
    end

    # `image` receives the restored host Image object, not a token.
    def image(img)
      @node["items"] << img.to_h
      nil
    end

    def group
      child = Section.new("group")
      @node["items"] << child
      child
    end

    def to_h
      @node.merge("items" => @node["items"].map { |i| i.is_a?(Section) ? i.to_h : i })
    end
  end

  # The card dialect: header (a leaf) plus body / footer sections.
  class Card
    def initialize = (@node = { "type" => "card" })

    def header(value)
      @node["header"] = value
      nil
    end

    def body = section("body")
    def footer = section("footer")

    def result
      @node.transform_values { |v| v.is_a?(Section) ? v.to_h : v }
    end

    private

    def section(slot)
      child = Section.new(slot)
      @node[slot] = child
      child
    end
  end

  # The backend factory bound at the guest's `Studio`: it mints fresh dialect
  # objects. `image` and `card` return stateful objects that cross as Handles.
  class Studio
    def card = Card.new
    def image(url:) = Image.new(url)
  end

  # Composes the guest idiom (`Build`) with the host backend (`Studio`). The
  # provider is callable, so a fresh factory backs the path every invocation.
  def self.extension
    Kobako::Extension.new(
      name: :CardDsl,
      source: BUILD_SOURCE,
      backend: Kobako::Extension::Backend.new(
        path: "Studio",
        provider: -> { Studio.new }
      )
    )
  end
end

sandbox = Kobako::Sandbox.new
sandbox.install(Dsl.extension)

card = sandbox.eval(Dsl::CARD_SCRIPT)

# Exercise the vocabulary bound so running the example proves it: a verb no
# host dialect defines must be refused, surfacing as a capability failure.
unknown =
  begin
    sandbox.eval(Dsl::UNKNOWN_VERB_SCRIPT)
    "NOT rejected — the host vocabulary bound is broken!"
  rescue Kobako::ServiceError
    "rejected"
  end

puts "host-owned DSL demo — the guest writes the builder, the host owns it"
puts
puts "guest DSL (receiver-less, every dialect lives on the host):"
puts <<~GUEST.gsub(/^/, "  ")
  logo = Studio.image(url: "...")   # leaf -> raw Handle
  Build.new(Studio.card).instance_eval do
    header "Welcome aboard"
    body do
      text "Thanks for joining."
      image logo                    # Handle passed as an argument (B-16)
      group { ... }                 # a nested section dialect
    end
  end
GUEST
puts
puts "host-assembled card (one Sandbox invocation, built entirely host-side):"
puts JSON.pretty_generate(card).gsub(/^/, "  ")
puts
puts "unknown verb — a method no host dialect defines:"
puts "  marquee \"...\" : #{unknown}   # bounded by the host method set (B-42)"
