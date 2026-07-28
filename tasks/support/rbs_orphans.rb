# frozen_string_literal: true

require "rbs"

# Readers behind +rake gate:rbs:orphans+: the declarations in +sig/+ that
# name nothing in the implementation.
#
# Steep checks Ruby against RBS and never the other way, so a declaration
# left behind by a deletion is invisible to it — the code it described is
# gone, and nothing reads the signature to notice. +rbs validate+ only
# holds the signatures to themselves. This closes that direction.
#
# Only declarations with a runtime counterpart are checkable: an instance
# variable, a type alias, and an interface describe shapes rather than
# definitions, so they are skipped rather than reported.
module KobakoRbsOrphans
  # Declaration nodes carrying a namespace their members belong to.
  NAMESPACES = [RBS::AST::Declarations::Module, RBS::AST::Declarations::Class].freeze
  private_constant :NAMESPACES

  module_function

  # Every checkable declaration across +sources+ (+{path => rbs text}+),
  # as +[[owner, name, kind, path], ...]+ — +owner+ the qualified module
  # or class it is declared in, +kind+ one of +:constant+, +:instance+,
  # +:singleton+, or +:singleton_instance+ (RBS's +self?.+, which defines
  # both).
  def declarations(sources)
    sources.flat_map { |path, text| collect(parse(path, text), [], path) }
  end

  # The subset of +declarations+ naming nothing in the loaded
  # implementation. Resolution runs against the calling process, so the
  # caller loads the library first.
  def orphans(declarations)
    declarations.reject { |owner, name, kind, _| defined_at?(owner, name, kind) }
  end

  # +declaration+ as a reader would write it: +Owner::CONST+, +Owner#method+,
  # or +Owner.method+.
  def spell(declaration)
    owner, name, kind, = declaration
    separator = { constant: "::", instance: "#", singleton: ".", singleton_instance: "." }.fetch(kind)
    "#{owner}#{separator}#{name}"
  end

  # Top-level declarations of one signature file.
  def parse(path, text)
    _, _, decls = RBS::Parser.parse_signature(RBS::Buffer.new(name: path, content: text))
    decls
  end
  private_class_method :parse

  # Walk +nodes+ under the qualified +scope+, gathering checkable
  # declarations. An interface has no runtime counterpart, so neither it
  # nor its members are gathered.
  def collect(nodes, scope, path)
    nodes.flat_map do |node|
      next [] if node.is_a?(RBS::AST::Declarations::Interface)

      found = declaration(node, scope, path)
      found ? [found] : descend(node, scope, path)
    end
  end
  private_class_method :collect

  # The checkable declaration +node+ carries, or +nil+ when it carries
  # none. An attribute reads like a method definition here because that is
  # what it defines.
  def declaration(node, scope, path)
    case node
    when RBS::AST::Declarations::Constant
      [scope.join("::"), node.name.name, :constant, path]
    when RBS::AST::Members::MethodDefinition, RBS::AST::Members::AttrReader
      [scope.join("::"), node.name, node.kind, path]
    end
  end
  private_class_method :declaration

  # Declarations inside +node+, under the scope its members resolve at —
  # a module or class extends that scope, anything else leaves it alone.
  def descend(node, scope, path)
    return [] unless node.respond_to?(:members)

    inner = NAMESPACES.any? { |type| node.is_a?(type) } ? scope + [node.name.to_s] : scope
    collect(node.members, inner, path)
  end
  private_class_method :descend

  # Whether +name+ of +kind+ exists on +owner+ in the loaded
  # implementation. Visibility is not part of the question — a private
  # method is declared in RBS and defined in Ruby just as a public one is.
  def defined_at?(owner, name, kind)
    mod = constant(owner)
    return false if mod.nil?

    case kind
    when :constant then !constant("#{owner}::#{name}").nil?
    when :singleton then mod.respond_to?(name, true)
    when :instance then instance_method?(mod, name)
    else mod.respond_to?(name, true) || instance_method?(mod, name)
    end
  end
  private_class_method :defined_at?

  # The constant at the qualified +path+, or +nil+ when nothing is there.
  # Each step is resolved without ancestor lookup so a name inherited from
  # a superclass does not answer for one the namespace never defined.
  def constant(path)
    path.split("::").inject(Object) { |namespace, name| namespace.const_get(name, false) }
  rescue NameError
    nil
  end
  private_class_method :constant

  # Whether +mod+ defines +name+ as an instance method at any visibility.
  def instance_method?(mod, name)
    return false unless mod.is_a?(Module)

    mod.method_defined?(name) || mod.private_method_defined?(name) || mod.protected_method_defined?(name)
  end
  private_class_method :instance_method?
end
