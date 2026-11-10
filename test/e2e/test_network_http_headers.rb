# frozen_string_literal: true

require "test_helper"

# E2E — the headers and form data a script's Net::HTTP request carries,
# through the real Guest Binary against a loopback server. Each expected value
# is what CRuby 3.4's net/http answers for the same script, except where the
# host writes a header itself.
class TestE2ENetworkHttpHeaders < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  DEFAULTS = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    [http.get("/").code, http.post("/", "a=1").code]
  RUBY

  # @behavior NW-102
  def test_a_request_carries_crubys_default_headers
    with_serving_server do |port, seen|
      http_sandbox(port).eval(format(DEFAULTS, port: port))
      get, post = [seen.pop, seen.pop].map { |head| fields_of(head) }

      assert_equal [["*/*", "ruby", nil], ["*/*", "ruby", "application/x-www-form-urlencoded"], [nil, nil]],
                   [get.values_at("accept", "user-agent", "content-type"),
                    post.values_at("accept", "user-agent", "content-type"),
                    [get["accept-encoding"], post["accept-encoding"]]],
                   "Net::HTTP#get and #post must send accept: */* and user-agent: Ruby, a POST body naming no " \
                   "content type must send application/x-www-form-urlencoded, and neither may send accept-encoding"
    end
  end

  # Each header a request head carries, its name and value lowercased.
  def fields_of(head)
    head.drop(1).to_h { |line| line.downcase.split(": ", 2) }
  end

  FIELDS = <<~RUBY
    request = Net::HTTP::Get.new("/", { "X-A" => " a ", "X-N" => nil, "x-c" => "1", "X-C" => "2" })
    request["X-B"] = %w[p q]
    refused = begin
      Net::HTTP::Get.new("/", { "x" => "a\\nb" })
    rescue ArgumentError => e
      e.message
    end
    [request["x-a"], request.key?("x-n"), request["x-c"], request["x-b"], refused]
  RUBY

  # @behavior NW-104
  def test_a_requests_headers_are_taken_as_cruby_takes_them
    value = http_sandbox(9).eval(FIELDS).value

    assert_equal ["a", false, "2", "p, q", "header x has field value \"a\\nb\", this cannot include CR/LF"], value,
                 "Net::HTTP::Get.new must strip a header value, skip a nil one, keep the last of a name in another " \
                 "case, join an Array set by []= with ', ', and raise ArgumentError for CR or LF"
  end

  REFUSED_HEADERS = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    [{ "Host" => "elsewhere" }, { "Connection" => "keep-alive" }, { "a b" => "c" }].map do |header|
      http.get("/", header)
    rescue ArgumentError => e
      e.message
    end
  RUBY

  # @behavior NW-105
  def test_a_header_the_host_writes_or_wasi_refuses_raises_argument_error
    with_serving_server do |port, seen|
      value = http_sandbox(port).eval(format(REFUSED_HEADERS, port: port)).value

      assert_equal [["header host cannot be set, as the host writes it",
                     "header connection cannot be set, as the host writes it",
                     "header a b is not a valid HTTP field"], 0], [value, seen.size],
                   "Net::HTTP#get naming host, connection, or a name that is no token must raise ArgumentError " \
                   "before anything is sent"
    end
  end

  FORM = <<~RUBY
    request = Net::HTTP::Post.new("/")
    answer = request.set_form_data({ "a" => %w[1 2], "b" => nil, c: "x y" }, ";")
    [answer, request.body, request.content_type]
  RUBY

  # @behavior NW-106
  def test_form_data_is_encoded_as_cruby_encodes_it
    value = http_sandbox(9).eval(FORM).value

    assert_equal %w[application/x-www-form-urlencoded a=1;a=2;b;c=x+y application/x-www-form-urlencoded], value,
                 "set_form_data must encode an Array value as one pair each, a nil value as its name, and a Symbol " \
                 "by its to_s, as URI.encode_www_form does, join by the separator, and answer the content type"
  end
end
