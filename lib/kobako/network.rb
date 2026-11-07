# frozen_string_literal: true

require "ipaddr"

module Kobako
  # The network a Sandbox gives the guest, one named entry at a time. An entry
  # is any object answering +endpoints+, or one Sandbox#network builds from its
  # block:
  #
  #   sandbox.network(:github) do |net|
  #     net.endpoint "api.github.com", port: 443
  #     net.endpoint "cache.internal", port: 6379, protocol: :tcp, addresses: [:private]
  #   end
  module Network
    # What the guest may reach: a +host+ and a +port+, the +protocol+ it
    # speaks, and the +addresses+ the host's name may lead to. Unless written
    # otherwise it speaks +:http+ and leads only to +:public+ addresses. Each
    # address is +:public+, +:private+, +:local+, or a CIDR as a String or an
    # IPAddr, kept as <tt>"address/prefix"</tt>.
    #
    # Raises ArgumentError for an empty host, a port outside 1 to 65535, a
    # protocol other than +:http+, +:tcp+, or +:udp+, or another address.
    class Endpoint < Data.define(:host, :port, :protocol, :addresses)
      PROTOCOLS = %i[http tcp udp].freeze # :nodoc:
      ADDRESS_SETS = %i[public private local].freeze # :nodoc:

      def initialize(host:, port:, protocol: :http, addresses: [:public])
        host = host.to_s.freeze
        refuse_malformed(host, port, protocol)
        addresses = Array(addresses).map { |address| to_address(address) }.freeze
        super
      end

      private

      def to_address(address)
        return address if ADDRESS_SETS.include?(address)

        cidr = address.is_a?(IPAddr) ? address : IPAddr.new(address.to_str)
        "#{cidr}/#{cidr.prefix}".freeze
      rescue IPAddr::Error, NoMethodError
        raise ArgumentError, "an address must be one of #{ADDRESS_SETS.inspect} or a CIDR, got #{address.inspect}"
      end

      def refuse_malformed(host, port, protocol)
        raise ArgumentError, "an endpoint's host must not be empty" if host.empty?
        unless (1..65_535).include?(port)
          raise ArgumentError,
                "an endpoint's port must be 1 to 65535, got #{port.inspect}"
        end
        return if PROTOCOLS.include?(protocol)

        raise ArgumentError, "protocol must be one of #{PROTOCOLS.inspect}, got #{protocol.inspect}"
      end
    end

    # One connection the guest asks for, as a +connect?+ policy is handed it:
    # the +host+ it named, a name or a literal address, its +port+, and its
    # +protocol+. The host is never a real address a name resolved to.
    class Connection < Data.define(:host, :port, :protocol); end

    # The entry a block given to Sandbox#network builds, adding to the object
    # given beside it, as Faraday.new yields its builder.
    class Builder
      HOOKS = %i[connect].freeze # :nodoc:

      def initialize(entry = nil) # :nodoc:
        @entry = entry
        @endpoints = [] # : Array[Endpoint]
        @connect = [] # : Array[^(Connection) -> boolish]
      end

      # Let the guest reach +host+ on +port+, as Endpoint.new takes them.
      # Returns +self+.
      def endpoint(host, port:, protocol: :http, addresses: [:public])
        @endpoints << Endpoint.new(host: host, port: port, protocol: protocol, addresses: addresses)
        self
      end

      # Judge each decision +hook+ names with the block; a falsy answer or a
      # raised StandardError refuses. +:connect+ hands the block a Connection.
      # Raises ArgumentError for another hook. Returns +self+.
      def policy(hook, &block)
        unless HOOKS.include?(hook)
          raise ArgumentError,
                "a policy hook must be one of #{HOOKS.inspect}, got #{hook.inspect}"
        end
        raise ArgumentError, "a policy needs a block" unless block

        @connect << block
        self
      end

      # The given object's endpoints, then the block's.
      def endpoints
        given = @entry.respond_to?(:endpoints) ? @entry.endpoints.to_a : [] # : Array[untyped]
        given + @endpoints
      end

      # Whether the given object's +connect?+ and each block's connect policy
      # allow +connection+.
      def connect?(connection)
        return false if @entry.respond_to?(:connect?) && !@entry.connect?(connection)

        @connect.all? { |policy| policy.call(connection) }
      end
    end
  end
end
