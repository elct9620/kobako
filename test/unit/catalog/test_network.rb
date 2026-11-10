# frozen_string_literal: true

require "test_helper"

module Kobako
  # Unit tests for a network entry as the Catalog holds it: one Grant per
  # name, judged only against its own kind, and its endpoints' defaults.
  # Behavioural coverage at the Sandbox#network boundary lives in
  # test/e2e/test_network.rb.
  class CatalogNetworkTest < Minitest::Test
    def setup
      @grants = Catalog::Grants.new
    end

    def entry(name, &builder)
      Catalog::Network.new(name: name, entry: nil, &builder)
    end

    # @behavior NW-002
    def test_a_name_given_twice_is_refused
      @grants.register(entry(:github))

      assert_raises(ArgumentError, "Sandbox#network of a name already given must be refused") do
        @grants.register(entry("github"))
      end
    end

    # @behavior NW-003
    def test_a_name_and_a_mount_path_never_meet
      @grants.register(Catalog::Mount.new(at: "/data", access: :read, filesystem: Filesystem::Tmpfs.new, policy: nil))
      @grants.register(entry("/data"))

      assert_equal [1, 1], @grants.entries.map(&:size),
                   "Sandbox#network naming a path Sandbox#mount took must stand beside the Mount"
    end

    # @behavior NW-007
    def test_an_endpoint_reaches_http_and_public_addresses_unless_written_otherwise
      endpoint = Network::Endpoint.new(host: "api.example.com", port: 443)

      assert_equal [:http, [:public]], [endpoint.protocol, endpoint.addresses],
                   "Kobako::Network::Endpoint.new with only a host and a port must speak HTTP to the public set"
    end

    MALFORMED_ENDPOINTS = [
      { host: "", port: 443 },
      { host: "api.example.com", port: 0 },
      { host: "api.example.com", port: 443, protocol: :ftp },
      { host: "api.example.com", port: 443, addresses: [:carrier] },
      { host: "api.example.com", port: 443, addresses: ["10.0.0.0/99"] }
    ].freeze

    # @behavior NW-008
    def test_an_empty_name_or_a_malformed_endpoint_is_refused
      assert_raises(ArgumentError, "Sandbox#network with an empty name must be refused") { entry("") }
      MALFORMED_ENDPOINTS.each do |fields|
        assert_raises(ArgumentError, "Kobako::Network::Endpoint.new(#{fields}) must be refused") do
          Network::Endpoint.new(**fields)
        end
      end
    end

    def test_a_cidr_given_as_a_string_or_an_ipaddr_is_kept_as_address_and_prefix
      endpoint = Network::Endpoint.new(host: "cache.internal", port: 6379,
                                       addresses: [:private, "10.1.2.3/8", IPAddr.new("192.168.0.0/16")])

      assert_equal [:private, "10.0.0.0/8", "192.168.0.0/16"], endpoint.addresses,
                   "Kobako::Network::Endpoint.new must keep an address set as it is and a CIDR as address/prefix"
    end

    def test_a_block_adds_to_the_object_given_beside_it
      given = Struct.new(:endpoints).new([Network::Endpoint.new(host: "a.example.com", port: 443)])
      network = Catalog::Network.new(name: :api, entry: given) { |net| net.endpoint "b.example.com", port: 443 }

      assert_equal %w[a.example.com b.example.com], network.entry.endpoints.map(&:host),
                   "an entry given as an object and a block must answer the object's endpoints, then the block's"
    end

    # An object refusing a connect to one host.
    RefusesHost = Struct.new(:refused) do
      def connect?(connection) = connection.host != refused
    end

    # @behavior NW-021
    def test_a_block_policy_and_the_given_objects_connect_both_judge
      builder = Catalog::Network.new(name: :guard, entry: RefusesHost.new("10.0.0.9")) do |net|
        net.policy(:connect) { |c| c.port != 22 }
      end.entry
      answers = [["10.0.0.1", 80], ["10.0.0.1", 22], ["10.0.0.9", 80]].map do |host, port|
        builder.connect?(Network::Connection.new(host: host, port: port, protocol: :tcp))
      end

      assert_equal [true, false, false], answers,
                   "Builder#connect? must refuse what the given object's connect? or a policy(:connect) block refuses"
    end

    # An object refusing a lookup of one name.
    RefusesName = Struct.new(:refused) do
      def resolve?(name) = name != refused
    end

    # @behavior NW-036
    def test_a_block_policy_and_the_given_objects_resolve_both_judge
      builder = Catalog::Network.new(name: :guard, entry: RefusesName.new("a.example.com")) do |net|
        net.policy(:resolve) { |name| !name.end_with?(".internal") }
      end.entry

      assert_equal [true, false, false], %w[b.example.com a.example.com db.internal].map { builder.resolve?(_1) },
                   "Builder#resolve? must refuse what the given object's resolve? or a policy(:resolve) block refuses"
    end

    def test_a_block_adds_credentials_after_the_given_objects
      given = Struct.new(:credentials).new([:given])
      builder = Catalog::Network.new(name: :api, entry: given) do |net|
        net.credential(:bearer, placeholder: "kobako-token") { "s3cret" }
      end.entry

      assert_equal [:given, :bearer, "kobako-token"],
                   [builder.credentials.first, builder.credentials.last.placement,
                    builder.credentials.last.placeholder],
                   "Builder#credential must add after the given object's credentials"
    end

    def test_a_credential_with_another_placement_is_refused
      assert_raises(ArgumentError, "Kobako::Network::Credential with another placement must be refused") do
        Kobako::Network::Credential.new(:digest, placeholder: "x") { "y" }
      end
    end

    def test_a_policy_for_an_unknown_hook_is_refused
      assert_raises(ArgumentError,
                    "Builder#policy for a hook other than :resolve, :connect, or :request must be refused") do
        Catalog::Network.new(name: :guard, entry: nil) { |net| net.policy(:listen) { true } }
      end
    end
  end
end
