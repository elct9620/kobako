# frozen_string_literal: true

require "test_helper"

# E2E — a script's Net::HTTP handing a response body to a destination, as
# net-http 0.6.0's `dest` argument does, through the real Guest Binary
# against a loopback server.
class TestE2ENetworkHttpDest < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  DESTS = <<~RUBY
    outcome = lambda do |&call|
      call.call
    rescue StandardError, NotImplementedError => e
      [e.class.to_s, e.message]
    end
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    got = +"<"
    response = http.get("/", nil, got)
    posted = +""
    http.post("/", "x", nil, posted)
    read = +""
    twice = nil
    http.request(Net::HTTP::Get.new("/")) do |answer|
      answer.read_body(read)
      twice = outcome.call { answer.read_body(+"") }
    end
    headless = +"x"
    head = http.request(Net::HTTP::Head.new("/")) { |answer| answer.read_body(headless) }
    both = outcome.call { http.get("/", nil, +"") { |_| } }
    proxy = outcome.call { Net::HTTP.start("127.0.0.1", nil, "proxy.example", 8080, use_ssl: true) }.first
    [got, response.body.equal?(got), posted, read, twice, [head.body, headless], both, proxy]
  RUBY

  # A loopback server answering each of +count+ requests with "hello".
  def with_hello_server(count)
    server = TCPServer.new("127.0.0.1", 0)
    thread = Thread.new { count.times { answer_hello(server.accept) } }
    yield server.addr[1]
  ensure
    thread&.join(5)
    server&.close
  end

  # Read +client+'s request head and answer "hello", closing after.
  def answer_hello(client)
    read_head(client)
    client.write("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nhello")
    client.close
  end

  # @behavior NW-192
  def test_a_response_body_goes_to_a_destination_as_net_https
    with_hello_server(5) do |port|
      value = http_sandbox(port).eval(format(DESTS, port: port)).value

      assert_equal ["<hello", true, "hello", "hello", ["IOError", "Net::HTTPOK#read_body called twice"],
                    [nil, "x"], ["ArgumentError", "both arg and block given for HTTP method"],
                    "NotImplementedError"], value,
                   "Net::HTTP#get and #post with a dest, and read_body(dest), must append the body to dest, which " \
                   "stands as the body; a second read_body(dest) must raise IOError and a HEAD answer's leave dest " \
                   "alone; a dest beside a block must raise ArgumentError; and Net::HTTP.start with use_ssl and a " \
                   "proxy must refuse the proxy, as net-http 0.6.0's do"
    end
  end
end
