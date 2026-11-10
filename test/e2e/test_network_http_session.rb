# frozen_string_literal: true

require "test_helper"

# E2E — the sessions, responses, and objects a script's Net::HTTP shares with
# CRuby, through the real Guest Binary against a loopback server, and where
# the subset stops. Each expected value is what CRuby 3.4's net/http answers
# for the same script, except where a scenario names the difference.
class TestE2ENetworkHttpSession < Minitest::Test
  include E2eGuestHelper
  include NetworkLoopback

  BODYLESS = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    chunks = []
    streamed = http.request(Net::HTTP::Head.new("/")) { |res| res.read_body { |chunk| chunks << chunk } }
    [http.head("/").body, http.get("/204").body, http.get("/304").body, chunks, streamed.body]
  RUBY

  # @behavior NW-103
  def test_a_response_that_carries_no_body_answers_nil
    with_serving_server do |port, _seen|
      value = http_sandbox(port).eval(format(BODYLESS, port: port)).value

      assert_equal [nil, nil, nil, [], nil], value,
                   "the body of a HEAD response and of a 204 or 304 response must answer nil, and read_body " \
                   "within a HEAD request's block must hand its block nothing"
    end
  end

  REFUSALS = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    twice = begin; http.start; http.start; rescue IOError => e; e.message; end
    http.finish
    unstarted = begin; Net::HTTP.new("127.0.0.1", %<port>d).finish; rescue IOError => e; e.message; end
    request = Net::HTTP::Post.new("/")
    request.body = "a"
    both = begin; http.request(request, "b"); rescue ArgumentError => e; e.message; end
    paths = [nil, ""].map { |path| begin; Net::HTTP::Get.new(path); rescue ArgumentError => e; e.message; end }
    [twice, unstarted, both, paths]
  RUBY

  # @behavior NW-107
  def test_a_session_and_a_request_refuse_what_crubys_refuse
    with_serving_server do |port, _seen|
      value = http_sandbox(port).eval(format(REFUSALS, port: port)).value

      assert_equal ["HTTP session already opened", "HTTP session not yet started",
                    "both of body argument and HTTPRequest#body set",
                    ["no HTTP request path given", "HTTP request path is empty"]], value,
                   "Net::HTTP#start twice and #finish unstarted must raise IOError, #request with a body beside " \
                   "the request's own must raise ArgumentError, and so must a nil or empty request path"
    end
  end

  OBJECTS = <<~RUBY
    plain = Net::HTTP.new("127.0.0.1")
    plain.use_ssl = true
    secure = Net::HTTP.start("127.0.0.1", use_ssl: true) { |http| [http.port, http.inspect] }
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    inside = nil
    response = http.request(Net::HTTP::Get.new("/")) { |res| inside = res.inspect }
    [plain.port, secure, http.inspect, Net::HTTP::Get.new("/").inspect, inside, response.inspect]
  RUBY

  # @behavior NW-108
  def test_net_http_objects_read_as_crubys
    with_serving_server do |port, _seen|
      value = http_sandbox(port).eval(format(OBJECTS, port: port)).value

      assert_equal [80, [443, "#<Net::HTTP 127.0.0.1:443 open=true>"], "#<Net::HTTP 127.0.0.1:#{port} open=false>",
                    "#<Net::HTTP::Get GET>", "#<Net::HTTPOK 200 OK readbody=false>",
                    "#<Net::HTTPOK 200 OK readbody=true>"], value,
                   "use_ssl= after Net::HTTP.new must keep port 80, Net::HTTP.start with use_ssl: true must take " \
                   "443, and each inspect must answer CRuby's form"
    end
  end

  CLASSES = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    ["/203", "/451", "/501", "/418"].map { |path| http.get(path).class.to_s } +
      [Net::HTTPRequestEntityTooLarge.equal?(Net::HTTPPayloadTooLarge), Net::HTTPNotImplemented.superclass.to_s]
  RUBY

  # @behavior NW-062
  def test_a_response_takes_the_class_crubys_net_http_gives_its_status
    with_serving_server do |port, _seen|
      value = http_sandbox(port).eval(format(CLASSES, port: port)).value

      assert_equal ["Net::HTTPNonAuthoritativeInformation", "Net::HTTPUnavailableForLegalReasons",
                    "Net::HTTPNotImplemented", "Net::HTTPClientError", true, "Net::HTTPServerError"], value,
                   "Net::HTTP#get must answer the class CRuby's net/http names for each status it knows, the " \
                   "family's for one it does not, and its older names as the same class"
    end
  end

  SUBSET = <<~RUBY
    http = Net::HTTP.new("127.0.0.1", %<port>d)
    uri = begin; URI("http://127.0.0.1/"); rescue NoMethodError; :no_uri; end
    enumerator = begin; Net::HTTP::Get.new("/").each_header; rescue NotImplementedError; :no_enumerator; end
    relative = begin; http.request(Net::HTTP::Get.new("items")); rescue ArgumentError => e; e.message; end
    [uri, enumerator, relative] + ["/204", "/203", "/451", "/505"].map { |path| http.get(path).message }
  RUBY

  # @behavior NW-109
  def test_net_http_is_a_subset_without_uri_or_enumerator
    with_serving_server do |port, _seen|
      value = http_sandbox(port).eval(format(SUBSET, port: port)).value

      assert_equal [:no_uri, :no_enumerator, "invalid path", "No Content", "Non-Authoritative Information",
                    "Unavailable For Legal Reasons", "HTTP Version Not Supported"], value,
                   "URI must raise NoMethodError, each_header with no block NotImplementedError, a relative path " \
                   "ArgumentError, and message must answer HTTP/1.1's reason phrase rather than the server's"
    end
  end
end
