# frozen_string_literal: true

require "test_helper"

# E2E — what an http endpoint's rules and a request policy let a script's
# Net::HTTP send, through the real Guest Binary against a loopback server.
class TestE2ENetworkHttpPolicy < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  SEND = <<~RUBY
    begin
      Net::HTTP.start("127.0.0.1", %<port>d) { |http| http.%<call>s }.code
    rescue Errno::EACCES
      :refused
    end
  RUBY

  def script(port, call) = format(SEND, port: port, call: call)

  # Send `call` through a Sandbox whose endpoint the block builds, then reach
  # the server once from the host so its accept returns whatever the guest did.
  def answer(port, call, &build)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).network(:web, &build)
    value = sandbox.eval(script(port, call)).value
    TCPSocket.open("127.0.0.1", port) { |socket| socket.write("GET / HTTP/1.1\r\n\r\n") } if value == :refused
    value
  end

  # The request a policy(:request) block is handed for a POST, and the code
  # the script reads back.
  def handed_post
    seen = []
    value = with_http_server do |port, _|
      answer(port, 'post("/items?x=1", "secret", { "X-Probe" => "yes" })') do |net|
        net.endpoint "127.0.0.1", port: port
        net.policy(:request) { |request| (seen << request) && false }
      end
    end
    [value, seen.first]
  end

  # @behavior NW-046
  def test_an_endpoints_rules_admit_only_the_methods_and_paths_they_allow
    answers = %w[get("/repos/a/b") post("/repos/a/b","x")].map do |call|
      with_http_server do |port, _|
        answer(port, call) { |net| net.endpoint("127.0.0.1", port: port) { |e| e.allow :get, "/repos/**" } }
      end
    end

    assert_equal ["201", :refused], answers,
                 "Net::HTTP through an endpoint allowing GET /repos/** must send a GET there and refuse a POST"
  end

  # @behavior NW-047
  def test_a_request_policy_is_handed_the_request_and_refuses_it
    value, request = handed_post

    assert_equal [:refused, "POST", "http", "/items", %w[x-probe]],
                 [value, request&.method, request&.scheme, request&.path, request&.header_names],
                 "a policy(:request) block must be handed the method, scheme, path with no query, and header " \
                 "names, and its refusal must raise Errno::EACCES"
  end
end
