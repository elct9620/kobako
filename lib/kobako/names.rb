# frozen_string_literal: true

# The name rules on Kobako itself, at the root tier so the Catalog and
# Transport share one.
module Kobako
  # A name the Host App gives is a Symbol or a String, as CRuby's
  # +const_get+ and +define_method+ take one; any other class is refused
  # before its text is read.
  def self.to_name(name, what) # :nodoc:
    unless name.is_a?(Symbol) || name.is_a?(String)
      raise TypeError,
            "#{what} must be a Symbol or String, got #{name.class}"
    end

    name.to_s
  end

  # A name that must read as a Ruby constant, as the Symbol it stands for.
  def self.to_constant_name(name, what) # :nodoc:
    text = to_name(name, what)
    unless constant_name?(text)
      raise ArgumentError,
            "#{what} must be a Ruby constant name (got #{name.inspect})"
    end

    text.to_sym
  end
end
