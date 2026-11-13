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
      assert [endpoint.host, endpoint.addresses, endpoint.rules].all?(&:frozen?),
             "Kobako::Network::Endpoint.new must hold its host, addresses, and rules frozen, as data"
    end

    MALFORMED_ENDPOINTS = [
      { host: "", port: 443 },
      { host: "api.example.com", port: 0 },
      { host: "api.example.com", port: 70_000 },
      { host: "api.example.com", port: 443, protocol: :ftp },
      { host: "api.example.com", port: 443, addresses: [:carrier] },
      { host: "api.example.com", port: 443, addresses: ["not-an-address"] }
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

    def test_a_rule_names_its_method_in_uppercase_or_nil_for_any
      rules = Network::Rules.new.allow(:get, "/repos/**").deny(:any, "/admin").to_a

      assert_equal [[true, "GET", "/repos/**"], [false, nil, "/admin"]],
                   rules.map { |rule| [rule.allow?, rule.method, rule.path] },
                   "Rules#allow :get must read back as GET, and Rules#deny :any as nil, the rule for any method"
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
