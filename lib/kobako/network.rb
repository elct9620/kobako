# frozen_string_literal: true

require "ipaddr"

module Kobako
  # The network a Sandbox gives the guest, one named entry at a time. An entry
  # is any object answering one or more of +endpoints+, +resolve?+,
  # +connect?+, +request?+, and +credentials+, or one Sandbox#network builds
  # from its block:
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
    # An +:http+ endpoint's +rules+ say which methods and paths it allows and
    # denies; with none, it allows every request.
    #
    # Raises ArgumentError for an empty host, a port outside 1 to 65535, a
    # protocol other than +:http+, +:tcp+, or +:udp+, or another address.
    class Endpoint < Data.define(:host, :port, :protocol, :addresses, :rules)
      PROTOCOLS = %i[http tcp udp].freeze # :nodoc:
      ADDRESS_SETS = %i[public private local].freeze # :nodoc:

      def initialize(host:, port:, protocol: :http, addresses: [:public], rules: [])
        host = host.to_s.freeze
        refuse_malformed(host, port, protocol)
        addresses = Array(addresses).map { |address| to_address(address) }.freeze
        rules = Array(rules).freeze
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

    # One rule an +:http+ endpoint judges a request by: whether it allows or
    # denies, its +method+ uppercase or +nil+ for any, and a +path+ pattern,
    # where <tt>*</tt> stands for one segment and <tt>**</tt> for any number.
    class HttpRule
      attr_reader :method, :path

      # Whether the rule allows what it matches, or denies it.
      def allow? = @allow

      def initialize(allow:, method:, path:)
        @allow = allow ? true : false
        @method = method.nil? || method == :any ? nil : method.to_s.upcase.freeze
        @path = path.to_s.freeze
        freeze
      end
    end

    # The rules a block given to Builder#endpoint adds to its endpoint.
    class Rules
      def initialize # :nodoc:
        @rules = [] # : Array[HttpRule]
      end

      # Allow +method+, or +:any+, on paths +path+ matches. Returns +self+.
      def allow(method, path)
        @rules << HttpRule.new(allow: true, method: method, path: path)
        self
      end

      # Deny +method+, or +:any+, on paths +path+ matches; a denial wins over
      # any allowance. Returns +self+.
      def deny(method, path)
        @rules << HttpRule.new(allow: false, method: method, path: path)
        self
      end

      def to_a = @rules.dup # :nodoc:
    end

    # A value the Host App hands no guest, swapped in place of +placeholder+ in
    # one header of a request bound to its entry's own endpoints. The
    # +placement+ is +:bearer+ or +:basic+ for the +authorization+ header, or a
    # header's name. The block answers the value for a Request, or +nil+ to
    # send nothing, as the request leaves.
    class Credential
      PLACEMENTS = %i[bearer basic].freeze # :nodoc:

      attr_reader :placement, :placeholder

      def initialize(placement, placeholder:, &source)
        unless PLACEMENTS.include?(placement) || placement.is_a?(String)
          raise ArgumentError, "a credential's placement must be :bearer, :basic, or a header name"
        end
        raise ArgumentError, "a credential needs a block answering its value" unless source

        @placement = placement.is_a?(String) ? placement.downcase.freeze : placement
        @placeholder = placeholder.to_s.freeze
        @source = source
      end

      # The value for +request+, or +nil+ to send nothing.
      def value(request) = @source.call(request)
    end

    # One HTTP request the guest sends, as a +request?+ policy is handed it:
    # the +host+ lowercase and the +port+, the +method+ uppercase, the
    # +scheme+, the +path+ with no query, and the +header_names+ lowercase,
    # never a header's value.
    class Request
      attr_reader :host, :port, :method, :scheme, :path, :header_names

      def initialize(authority, method, scheme, path, header_names) # :nodoc:
        @host, @port = authority
        @method = method
        @scheme = scheme
        @path = path
        @header_names = header_names
        freeze
      end
    end

    # The entry a block given to Sandbox#network builds, adding to the object
    # given beside it, as Faraday.new yields its builder.
    class Builder
      HOOKS = %i[resolve connect request].freeze # :nodoc:

      def initialize(entry = nil) # :nodoc:
        @entry = entry
        @endpoints = [] # : Array[Endpoint]
        @policies = HOOKS.to_h { |hook| [hook, []] } # : Hash[Symbol, Array[^(untyped) -> boolish]]
        @credentials = [] # : Array[Credential]
      end

      # Let the guest reach +host+ on +port+, as Endpoint.new takes them.
      # Returns +self+.
      # A given block yields a Rules adding the methods and paths an +:http+
      # endpoint allows and denies.
      def endpoint(host, port:, protocol: :http, addresses: [:public])
        rules = Rules.new
        yield rules if block_given?
        @endpoints << Endpoint.new(host: host, port: port, protocol: protocol, addresses: addresses,
                                   rules: rules.to_a)
        self
      end

      # Judge each decision +hook+ names with the block; a falsy answer or a
      # raised StandardError refuses. +:resolve+ hands the block the name a
      # lookup asks for, lowercase with no trailing dot, +:connect+ a
      # Connection, and +:request+ a Request. Raises ArgumentError for another
      # hook. Returns +self+.
      def policy(hook, &block)
        unless HOOKS.include?(hook)
          raise ArgumentError,
                "a policy hook must be one of #{HOOKS.inspect}, got #{hook.inspect}"
        end
        raise ArgumentError, "a policy needs a block" unless block

        @policies.fetch(hook) << block
        self
      end

      # The given object's endpoints, then the block's.
      def endpoints
        given = @entry.respond_to?(:endpoints) ? @entry.endpoints.to_a : [] # : Array[untyped]
        given + @endpoints
      end

      # Swap a Credential's value in place of +placeholder+ in requests bound
      # to this entry's endpoints. Returns +self+.
      def credential(placement, placeholder:, &source)
        @credentials << Credential.new(placement, placeholder: placeholder, &source)
        self
      end

      # The given object's credentials, then the block's.
      def credentials
        given = @entry.respond_to?(:credentials) ? @entry.credentials.to_a : [] # : Array[untyped]
        given + @credentials
      end

      # Whether the given object's +resolve?+ and each block's resolve policy
      # allow a lookup of +name+.
      def resolve?(name) = allows?(:resolve, name)

      # Whether the given object's +connect?+ and each block's connect policy
      # allow +connection+.
      def connect?(connection) = allows?(:connect, connection)

      # Whether the given object's +request?+ and each block's request policy
      # allow +request+.
      def request?(request) = allows?(:request, request)

      private

      def allows?(hook, asked)
        question = :"#{hook}?"
        return false if @entry.respond_to?(question) && !@entry.public_send(question, asked)

        @policies.fetch(hook).all? { |policy| policy.call(asked) }
      end
    end
  end
end
