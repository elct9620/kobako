# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — host-object DSL composition through real mruby. A guest-side
# builder idiom (a generic wrapper over Capability Handles) composes existing
# anchors into a nested-structure builder whose dialects live entirely on the
# host: a Service returns child Handles (B-14), the guest chains method calls
# onto them (B-17), and a guest-LOCAL wrapper drives the descent with
# +instance_eval+ or an explicit block parameter. The reflection denial is
# scoped to guest→host dispatch and to bound-constant / Handle proxies (B-42 / B-44),
# so +instance_eval+ on a plain guest-local object is permitted; the host
# dialect's own method set stays the reachable surface (B-42 undefined).
#
# These are witness tests: they pin an already-working composition as contract
# so a later change to the Handle lifecycle, the reflection boundary, or the
# guest proxy cannot silently break the host-object DSL pattern.
class TestE2EDslComposition < Minitest::Test
  include E2eGuestHelper

  # A two-level host dialect tree. +node+ returns the child (a fresh Handle
  # per B-14) so the guest can descend into it; +text+ is a leaf; +result+
  # serializes the whole host-held tree to a wire-representable Hash.
  class Node
    def initialize(name)
      @name = name
      @children = []
    end

    def node(name)
      child = Node.new(name)
      @children << child
      child
    end

    def text(value)
      @children << { "text" => value }
      nil
    end

    def result
      { @name => @children.map { |c| c.respond_to?(:result) ? c.result : c } }
    end
  end

  # The factory Service: +root+ returns the document's root Node as a Handle.
  class Builder
    def root(name)
      Node.new(name)
    end
  end

  EXPECTED = {
    "doc" => [
      { "body" => [{ "text" => "hello" }, { "inner" => [{ "text" => "deep" }] }] }
    ]
  }.freeze

  # The generic implicit-self wrapper: a plain guest-local class whose
  # +method_missing+ forwards a dynamic method name onto its Handle and,
  # when a block is given, descends into the returned child Handle via
  # +instance_eval+ — so the guest writes a receiver-less DSL.
  IMPLICIT_IDIOM = <<~RUBY
    class D
      def initialize(handle); @handle = handle; end
      def method_missing(name, *args, &blk)
        child = @handle.public_send(name, *args)
        if blk
          (child.is_a?(Kobako::Handle) ? D.new(child) : child).instance_eval(&blk)
          self
        else
          child
        end
      end
      def respond_to_missing?(_name, _include_private = false); true; end
      def handle; @handle; end
    end
  RUBY

  # The generic block-parameter wrapper: identical except it yields the
  # wrapped child to an explicit block parameter instead of rebinding self.
  BLOCK_PARAM_IDIOM = <<~RUBY
    class D
      def initialize(handle); @handle = handle; end
      def method_missing(name, *args, &blk)
        child = @handle.public_send(name, *args)
        if blk
          blk.call(child.is_a?(Kobako::Handle) ? D.new(child) : child)
          self
        else
          child
        end
      end
      def respond_to_missing?(_name, _include_private = false); true; end
      def handle; @handle; end
    end
  RUBY

  IMPLICIT_SCRIPT = <<~RUBY.freeze
    #{IMPLICIT_IDIOM}
    root = D.new(Builder.root("doc"))
    root.instance_eval do
      node("body") do
        text "hello"
        node("inner") do
          text "deep"
        end
      end
    end
    root.handle.result
  RUBY

  BLOCK_PARAM_SCRIPT = <<~RUBY.freeze
    #{BLOCK_PARAM_IDIOM}
    root = D.new(Builder.root("doc"))
    root.node("body") do |body|
      body.text "hello"
      body.node("inner") do |inner|
        inner.text "deep"
      end
    end
    root.handle.result
  RUBY

  # A name the host Node dialect does not define, forwarded through the
  # generic wrapper.
  UNBOUND_VOCAB_SCRIPT = <<~RUBY.freeze
    #{IMPLICIT_IDIOM}
    root = D.new(Builder.root("doc"))
    root.instance_eval { paragraph "not a Node method" }
    root.handle.result
  RUBY

  # B-42 / B-44 scope + B-14 / B-17: a receiver-less DSL driven by
  # +instance_eval+ on a guest-local wrapper builds a nested structure whose
  # dialects live on the host. Proves guest-local instance_eval is outside the
  # reflection denial and that returned child Handles chain to arbitrary depth.
  def test_implicit_self_idiom_builds_nested_structure_over_handles
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Builder", Builder.new)

    assert_equal EXPECTED, sandbox.eval(IMPLICIT_SCRIPT),
                 "an instance_eval-driven guest idiom over host-returned child Handles " \
                 "must build the nested structure host-side (B-14 / B-17; guest-local " \
                 "instance_eval permitted per the B-42 / B-44 dispatch-scoped denial)"
  end

  # Same composition through the explicit block-parameter form: the wrapper
  # yields the wrapped child rather than rebinding self.
  def test_block_param_idiom_builds_nested_structure_over_handles
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Builder", Builder.new)

    assert_equal EXPECTED, sandbox.eval(BLOCK_PARAM_SCRIPT),
                 "a block-parameter guest idiom over host-returned child Handles must " \
                 "build the same nested structure host-side (B-14 / B-17)"
  end

  # B-42: the generic forwarder does not widen the reachable surface — a
  # method the host dialect does not define, forwarded through the wrapper,
  # is refused host-side as an undefined target, surfacing as ServiceError.
  # The host dialect's own method set stays the DSL's vocabulary.
  def test_dialect_vocabulary_is_bounded_by_the_host_method_set
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Builder", Builder.new)

    assert_raises(Kobako::ServiceError,
                  "a name the host dialect does not define, forwarded through the " \
                  "generic wrapper, must be refused host-side (B-42 undefined) — the " \
                  "wrapper cannot reach beyond the dialect's own methods") do
      sandbox.eval(UNBOUND_VOCAB_SCRIPT)
    end
  end
end
