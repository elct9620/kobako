# frozen_string_literal: true

require "test_helper"

# E2E — the Net::HTTP surface a script shares with CRuby, through the real
# Guest Binary against a loopback server. Each expected value is what CRuby
# 3.4's net/http answers for the same script.
class TestE2ENetworkHttpCruby < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  STREAMED_BODY = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    chunks = []
    response = http.request(Net::HTTP::Get.new("/")) do |res|
      res.read_body { |chunk| chunks << chunk }
      begin
        res.read_body { |_| }
      rescue IOError => e
        chunks << e.message
      end
    end
    [chunks.first(chunks.size - 1).join, chunks.last, response.code, response.body]
  RUBY

  # @behavior NW-061
  def test_read_body_streams_the_body_within_a_requests_block
    with_http_server do |port, _seen|
      value = http_sandbox(port).eval(format(STREAMED_BODY, port: port)).value

      assert_equal ["ok", "Net::HTTPCreated#read_body called twice", "201", nil], value,
                   "read_body with a block inside Net::HTTP#request's block must hand over the body, a second " \
                   "read_body must raise IOError as CRuby's does, and body after it must answer nil"
    end
  end

  LARGE_BODY = <<~RUBY
    total = 0
    Net::HTTP.new("127.0.0.1", %<port>d).get("/") { |chunk| total += chunk.bytesize }
    total
  RUBY

  # @behavior NW-061
  def test_a_body_past_the_guests_memory_streams_through_a_block
    size = 8 << 20
    with_answering_server("HTTP/1.1 200 OK\r\ncontent-length: #{size}\r\n\r\n#{"x" * size}") do |port|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 4 << 20)
                               .network(:web) { |net| net.endpoint "127.0.0.1", port: port }

      assert_equal size, sandbox.eval(format(LARGE_BODY, port: port)).value,
                   "Net::HTTP#get with a block must stream a body twice the guest's memory_limit chunk by chunk"
    end
  end

  HELPERS = <<~RUBY
    req = Net::HTTP::Post.new("/form")
    req.basic_auth("u", "p")
    req.set_form_data("a" => "1 2", "b" => "&")
    shape = [req.content_type, req["authorization"], req.body, req.key?("Content-Type"), req.delete("x-none")]
    res = Net::HTTP.new("127.0.0.1", %<port>d).request(req)
    shape + [res.value, res.content_type, res.content_length, res.key?("X-Kind"), res.header.equal?(res),
             res.http_version, res.code_type.to_s]
  RUBY

  SESSION_SHAPE = <<~RUBY
    class Session < Net::HTTP; end
    session = Session.new("127.0.0.1", "%<port>d")
    response = session.get("/")
    [session.inspect, response.code, response.body { |_| raise "body passed its block" }]
  RUBY

  # @behavior NW-208
  def test_a_session_reads_its_port_inspect_and_body_as_crubys
    with_http_server do |port, _seen|
      assert_equal ["#<Session 127.0.0.1:#{port} open=false>", "201", "ok"],
                   http_sandbox(port).eval(format(SESSION_SHAPE, port: port)).value,
                   "Net::HTTP given its port as a String must reach the server, inspect must name the subclass " \
                   "and the port as text, and body with a block must pass none and answer the body"
    end
  end

  # @behavior NW-062
  def test_request_and_response_helpers_answer_as_cruby
    with_http_server do |port, seen|
      value = http_sandbox(port).eval(format(HELPERS, port: port)).value
      head, body = seen.pop

      assert_equal ["application/x-www-form-urlencoded", "Basic dTpw", "a=1+2&b=%26", true, nil,
                    nil, nil, 13, true, true, "1.1", "Net::HTTPCreated", "a=1+2&b=%26",
                    "Basic dTpw"],
                   value + [body, head.find { |h| h.downcase.start_with?("authorization:") }&.split(": ", 2)&.last],
                   "basic_auth, set_form_data, content_type, key?, delete, value, content_length, header, " \
                   "http_version, and code_type must answer as CRuby's net/http does"
    end
  end

  FAILED_RESPONSE = <<~RUBY
    begin
      Net::HTTP.new("127.0.0.1", %<port>d).get("/").value
    rescue Net::HTTPExceptions => e
      [e.class.to_s, e.message, e.response.code, e.is_a?(Net::ProtoServerError), e.is_a?(Net::ProtocolError)]
    end
  RUBY

  # @behavior NW-063
  def test_value_raises_the_error_cruby_raises_for_the_family
    with_answering_server("HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n") do |port|
      assert_equal ["Net::HTTPClientException", "404 \"Not Found\"", "404", true, true],
                   http_sandbox(port).eval(format(FAILED_RESPONSE, port: port)).value,
                   "Net::HTTPResponse#value of a 404 must raise Net::HTTPClientException carrying the response"
    end
  end
end
