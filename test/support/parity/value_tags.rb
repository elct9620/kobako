# frozen_string_literal: true

module Parity
  # Lossless tagged-JSON form of a decoded wire value, byte-identical
  # across executors: integers ride as strings, binary as hex, map
  # order preserved — so a plain +assert_equal+ over parsed JSON is the
  # whole comparison.
  module ValueTags
    module_function

    def tag(value)
      case value
      when Array then { "t" => "array", "v" => value.map { |item| tag(item) } }
      when Hash then { "t" => "map", "v" => value.map { |k, v| [tag(k), tag(v)] } }
      when String then tag_string(value)
      else tag_scalar(value)
      end
    end

    def tag_scalar(value)
      case value
      when nil then { "t" => "nil" }
      when true, false then { "t" => "bool", "v" => value }
      when Integer then { "t" => "int", "v" => value.to_s }
      when Float then { "t" => "float", "v" => value }
      when Symbol then { "t" => "sym", "v" => value.to_s }
      else { "t" => "unrepresentable", "class" => value.class.name }
      end
    end

    def tag_string(value)
      if value.encoding == Encoding::ASCII_8BIT
        { "t" => "bin", "hex" => value.unpack1("H*") }
      else
        { "t" => "str", "v" => value }
      end
    end

    # Inverse of +tag+ for the scenario constants a +"value"+ stub
    # behavior returns.
    def untag(tagged)
      tagged = tagged.transform_keys(&:to_s)
      case tagged.fetch("t")
      when "array" then tagged.fetch("v").map { |item| untag(item) }
      when "map" then tagged.fetch("v").to_h { |k, v| [untag(k), untag(v)] }
      else untag_scalar(tagged)
      end
    end

    def untag_scalar(tagged)
      case tagged.fetch("t")
      when "nil" then nil
      when "bool", "float", "str" then tagged.fetch("v")
      when "int" then Integer(tagged.fetch("v"))
      when "sym" then tagged.fetch("v").to_sym
      when "bin" then [tagged.fetch("hex")].pack("H*")
      else raise ArgumentError, "malformed tagged value: #{tagged.inspect}"
      end
    end
  end
end
