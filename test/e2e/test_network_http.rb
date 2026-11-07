# frozen_string_literal: true

require "test_helper"

# E2E — a script's Net::HTTP through the real Guest Binary against a loopback
# HTTP server: what an http endpoint admits, and what CRuby raises otherwise.
class TestE2ENetworkHttp < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  def sandbox_reaching(port)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:web) { |net| net.endpoint "127.0.0.1", port: port }
  end

  GET = <<~RUBY
    response = Net::HTTP.new("127.0.0.1", %<port>d).get("/greeting?lang=en", { "X-Probe" => "yes" })
    [response.code, response.message, response.body, response["X-Kind"],
     response.class.to_s, response.is_a?(Net::HTTPSuccess)]
  RUBY

  POST = <<~RUBY
    Net::HTTP.start("127.0.0.1", %<port>d) { |http| http.post("/items", "hello") }.body
  RUBY

  REFUSED = <<~RUBY
    begin
      Net::HTTP.get("127.0.0.1", "/", %<port>d)
    rescue SystemCallError, NotImplementedError => e
      [e.class.to_s, e.message]
    end
  RUBY

  # @behavior NW-042
  def test_a_get_reaches_a_server_an_http_endpoint_names
    with_http_server do |port, seen|
      value = sandbox_reaching(port).eval(format(GET, port: port)).value
      head, = seen.pop

      assert_equal [["201", "Created", "ok", "test", "Net::HTTPCreated", true], "GET /greeting?lang=en HTTP/1.1",
                    "yes"],
                   [value, head.first, head.find { |h| h.downcase.start_with?("x-probe:") }&.split(": ")&.last],
                   "Net::HTTP#get to a server an http endpoint names must send the path and headers and " \
                   "read back the code, message, body, and headers"
    end
  end

  # @behavior NW-042
  def test_a_post_sends_its_body
    with_http_server do |port, seen|
      value = sandbox_reaching(port).eval(format(POST, port: port)).value

      assert_equal %w[okhello hello], [value, seen.pop.last],
                   "Net::HTTP#post must send its body and read back the response's"
    end
  end

  # @behavior NW-043
  def test_a_request_no_http_endpoint_names_raises_eacces
    value = sandbox_reaching(1).eval(format(REFUSED, port: 2)).value

    assert_equal ["Errno::EACCES",
                  "Failed to open TCP connection to 127.0.0.1:2 " \
                  "(Permission denied - connect(2) for \"127.0.0.1\" port 2)"], value,
                 "Net::HTTP to an authority no http endpoint names must raise Errno::EACCES as CRuby wraps it"
  end

  # @behavior NW-043
  def test_without_the_network_net_http_raises_not_implemented_error
    value = Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(format(REFUSED, port: 80)).value

    assert_equal "NotImplementedError", value&.first,
                 "Net::HTTP in a Sandbox with no network entry must raise NotImplementedError"
  end
end
