# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/rbs_orphans"

# Unit coverage for the signature-orphan readers: declarations carry the
# namespace they resolve under, only the kinds with a runtime counterpart
# are gathered, and resolution asks whether a definition exists rather
# than whether it is reachable from outside.
class KobakoRbsOrphansTest < Minitest::Test
  Orphans = KobakoRbsOrphans

  # A namespace holding one of every shape resolution has to answer for:
  # a constant, a hidden constant, methods at both visibilities, a
  # singleton method, and an attribute.
  module Fixture
    LIVE = 1
    HIDDEN = 2
    private_constant :HIDDEN

    attr_reader :attribute

    def self.singleton_side; end

    def instance_side; end

    def hidden_side; end
    private :hidden_side

    def either_side; end
    module_function :either_side
  end

  # A constant reached only through a superclass — the case that decides
  # whether resolution walks ancestors.
  class Parent
    INHERITED = 1
  end

  class Child < Parent; end

  METHOD_FORMS = <<~RBS
    module Probe
      def plain: () -> void
      def self.only_singleton: () -> void
      def self?.both: () -> void
      attr_reader named: String
    end
  RBS

  SHAPES_WITHOUT_DEFINITIONS = <<~RBS
    module Probe
      @slot: Integer
      type name = String
    end
    interface _Encodable
      def encode: () -> String
    end
  RBS

  # Every live shape in Fixture, as +[name, kind]+.
  LIVE_SHAPES = [
    %i[LIVE constant], %i[HIDDEN constant],
    %i[instance_side instance], %i[hidden_side instance], %i[attribute instance],
    %i[singleton_side singleton], %i[either_side singleton_instance]
  ].freeze

  def declarations(text)
    Orphans.declarations({ "probe.rbs" => text })
  end

  # A declaration of +name+ under +owner+, resolved against this test's
  # own namespace so the fixtures above are what resolution answers from.
  def declaration(owner, name, kind)
    ["#{self.class.name}::#{owner}", name, kind, "probe.rbs"]
  end

  def test_a_nested_constant_is_qualified_by_the_namespace_it_sits_in
    found = declarations("module Outer\n  module Inner\n    WIDTH: Integer\n  end\nend\n")

    assert_equal [["Outer::Inner", :WIDTH, :constant, "probe.rbs"]], found,
                 "a constant nested two modules deep through .declarations must carry the qualified namespace " \
                 "it resolves under, not its bare name"
  end

  def test_a_namespace_written_as_one_path_qualifies_the_same_way
    found = declarations("module Outer::Inner\n  WIDTH: Integer\nend\n")

    assert_equal ["Outer::Inner"], found.map(&:first),
                 "a namespace spelled `module Outer::Inner` through .declarations must qualify its members the " \
                 "same as the nested spelling, since the two describe one namespace"
  end

  def test_each_method_form_records_the_kind_that_decides_where_it_is_looked_up
    kinds = declarations(METHOD_FORMS).to_h { |_, name, kind, _| [name, kind] }

    assert_equal({ plain: :instance, only_singleton: :singleton, both: :singleton_instance, named: :instance },
                 kinds,
                 "each method form through .declarations must record the kind it was written as, because that " \
                 "is what decides whether a definition is looked for on the singleton, the instances, or either")
  end

  def test_shapes_with_no_runtime_counterpart_are_not_gathered
    assert_empty declarations(SHAPES_WITHOUT_DEFINITIONS),
                 "an instance variable, a type alias, and an interface through .declarations must be gathered " \
                 "as nothing, since each describes a shape rather than a definition to find"
  end

  def test_a_declaration_naming_nothing_is_an_orphan
    found = Orphans.orphans([declaration("Fixture", :vanished, :instance)])

    assert_equal [:vanished], found.map { |orphan| orphan[1] },
                 "a method declaration whose implementation was deleted through .orphans must be reported, " \
                 "which is the drift no other check in the stack reads"
  end

  def test_a_namespace_that_does_not_exist_orphans_everything_declared_under_it
    found = Orphans.orphans([declaration("NoSuchNamespace", :LIVE, :constant)])

    assert_equal 1, found.size,
                 "a declaration under a namespace that no longer exists through .orphans must be reported " \
                 "rather than skipped, since a vanished namespace takes its members with it"
  end

  def test_every_live_shape_resolves_whatever_its_visibility
    live = LIVE_SHAPES.map { |name, kind| declaration("Fixture", name, kind) }

    assert_empty Orphans.orphans(live),
                 "a private constant and a private method through .orphans must resolve like public ones, " \
                 "because RBS declares a definition and visibility is not what it is declaring"
  end

  def test_an_inherited_constant_does_not_answer_for_the_namespace_that_never_defined_it
    found = Orphans.orphans([declaration("Child", :INHERITED, :constant)])

    assert_equal 1, found.size,
                 "a constant declared on a subclass but defined only on its parent through .orphans must be " \
                 "reported, so a constant that moved namespaces is caught instead of answered for by ancestry"
  end

  def test_each_kind_is_spelled_the_way_a_reader_would_write_it
    spelled = [
      ["Kobako::Codec", :EXT_FAULT, :constant, "probe.rbs"],
      ["Kobako::Pool", :checkout, :instance, "probe.rbs"],
      ["Kobako::Handle", :restore, :singleton, "probe.rbs"]
    ].map { |candidate| Orphans.spell(candidate) }

    assert_equal ["Kobako::Codec::EXT_FAULT", "Kobako::Pool#checkout", "Kobako::Handle.restore"], spelled,
                 "each declaration kind through .spell must read as Ruby writes it, so a reported orphan is " \
                 "searchable in the sources it went missing from"
  end
end
