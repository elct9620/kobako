# frozen_string_literal: true

require "test_helper"

# E2E — the Net::HTTP edges a script shares with CRuby, through the real Guest
# Binary against a loopback server: a body cut short, the verbs that take no
# block, the header setters, and where a session connects. Each expected value
# is what CRuby 3.4's net/http answers for the same script.
class TestE2ENetworkHttpEdges < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  # A loopback server answering every request with `response`, handing back
  # the request line and headers each one carried.
  def with_repeating_server(response)
    server = TCPServer.new("127.0.0.1", 0)
    seen = Queue.new
    thread = Thread.new { loop { answer_and_record(server.accept, response, seen) } }
    yield server.addr[1], seen
  ensure
    thread&.kill
    server&.close
  end

  def answer_and_record(client, response, seen)
    seen << read_head(client)
    client.write(response)
    client.close
  end

  CUT_BODY = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    begin
      http.get("/").body
    rescue EOFError => e
      [e.class.to_s, e.message]
    end
  RUBY

  # @behavior NW-119
  def test_a_response_body_cut_short_reads_as_crubys
    answers = ["content-length: 10\r\n\r\nabc", "transfer-encoding: chunked\r\n\r\n3\r\nabc\r\n"].map do |rest|
      with_answering_server("HTTP/1.1 200 OK\r\n#{rest}") do |port|
        http_sandbox(port).eval(format(CUT_BODY, port: port)).value
      end
    end

    assert_equal ["abc", ["EOFError", "end of file reached"]], answers,
                 "Net::HTTP#get of a body its content-length bounds that closes early must answer what arrived, " \
                 "and of a chunked body that closes early raise EOFError, as CRuby's ignore_eof reads each"
  end

  VERBS = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    chunks = []
    bodies = [http.delete("/") { |c| chunks << c }, http.options("/") { |c| chunks << c },
              http.put("/", "x") { |c| chunks << c }].map(&:body)
    inside = nil
    response = http.request(Net::HTTP::Get.new("/")) { inside = http.started? }
    request = Net::HTTP::Get.new("/")
    auth = request.basic_auth("u", "p")
    unset = begin; request.content_type = nil; rescue NoMethodError => e; e.message; end
    session = Net::HTTP.new("127.0.0.1", %<port>d)
    session.start
    ssl = begin; session.use_ssl = true; rescue IOError => e; e.message; end
    [bodies, chunks, inside, response.content_length, response.content_type, auth, unset, ssl]
  RUBY

  VERBS_IN_CRUBY = [%w[hello hello hello], [], true, 5, "text/html", ["Basic dTpw"],
                    "undefined method '+' for nil", "use_ssl value changed, but session already started"].freeze

  # @behavior NW-120
  def test_net_http_verbs_and_setters_answer_as_crubys
    head = "HTTP/1.1 200 OK\r\ncontent-length: 5, 5\r\ncontent-type: text / html\r\n\r\nhello"
    with_repeating_server(head) do |port, seen|
      value = http_sandbox(port).eval(format(VERBS, port: port)).value

      depth = seen.pop.find { |line| line.downcase.start_with?("depth:") }&.split(": ", 2)&.last

      assert_equal [VERBS_IN_CRUBY, "Infinity"], [value, depth],
                   "Net::HTTP#delete, #options, and #put must read their bodies whole, delete send Depth: Infinity, " \
                   "started? answer true inside a request, and the headers and setters answer as CRuby's"
    end
  end

  UNCONNECTED_SESSION = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    started = http.start.started?
    sent = begin; http.get("/"); :sent; rescue SystemCallError => e; e.class.to_s; end
    proxy = begin; Net::HTTP.new("127.0.0.1", %<port>d, "proxy.example", 8080); rescue NotImplementedError; :refused; end
    [started, sent, proxy]
  RUBY

  # @behavior NW-121
  def test_a_net_http_session_connects_at_its_first_request
    closed = TCPServer.new("127.0.0.1", 0)
    port = closed.addr[1]
    closed.close

    value = http_sandbox(port).eval(format(UNCONNECTED_SESSION, port: port)).value

    assert_equal [true, "Errno::ECONNREFUSED", :refused], value,
                 "Net::HTTP#start on a port nothing listens on must pass and the first request raise, and a " \
                 "proxy raise NotImplementedError"
  end

  ARITY = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    [-> { http.get("/", nil, nil, 1) }, -> { http.post("/", "d", nil, nil, 1) }].map do |call|
      call.call
    rescue ArgumentError => e
      e.message
    end
  RUBY

  # @behavior NW-198
  def test_a_net_http_verb_names_the_count_it_takes
    value = http_sandbox(1).eval(format(ARITY, port: 1)).value

    assert_equal ["wrong number of arguments (given 4, expected 1..3)",
                  "wrong number of arguments (given 5, expected 2..4)"], value,
                 "Net::HTTP#get with four arguments and #post with five must raise ArgumentError naming 1..3 and " \
                 "2..4, as CRuby's net/http does"
  end
end
