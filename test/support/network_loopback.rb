# frozen_string_literal: true

require "socket"

# A loopback server for the network journeys under test/e2e/, and a Sandbox
# whose one TCP endpoint reaches it.
module NetworkLoopback
  # A loopback server answering what it reads with its upcase, on a Ruby
  # Thread the guest's waits must let run.
  def with_upcase_server
    server = TCPServer.new("127.0.0.1", 0)
    thread = Thread.new do
      client = server.accept
      client.write(client.read.upcase)
      client.close
    end
    yield server.addr[1]
  ensure
    thread&.join(5)
    server&.close
  end

  # A loopback HTTP server answering one request with `created` and the body
  # it read, handing back the request line and headers it saw.
  def with_http_server
    server = TCPServer.new("127.0.0.1", 0)
    seen = Queue.new
    thread = Thread.new { serve_http_once(server, seen) }
    yield server.addr[1], seen
  ensure
    thread&.join(5)
    server&.close
  end

  def serve_http_once(server, seen)
    client = server.accept
    head = read_head(client)
    length = head.find { |h| h.downcase.start_with?("content-length:") }.to_s.split(":").last.to_i
    body = length.positive? ? client.read(length) : ""
    seen << [head, body]
    client.write("HTTP/1.1 201 Created\r\ncontent-length: #{body.bytesize + 2}\r\nx-kind: test\r\n\r\nok#{body}")
    client.close
  end

  # A loopback server answering one request with +response+, written whole.
  def with_answering_server(response)
    server = TCPServer.new("127.0.0.1", 0)
    thread = Thread.new { answer_once(server, response) }
    yield server.addr[1]
  ensure
    thread&.join(10)
    server&.close
  end

  # A loopback server that only listens: the backlog takes each connect.
  def with_listening_server
    server = TCPServer.new("127.0.0.1", 0)
    yield server.addr[1]
  ensure
    server&.close
  end

  # Yield the port of a loopback peer that answers a datagram upcased.
  def with_upcase_peer
    peer = UDPSocket.new
    peer.bind("127.0.0.1", 0)
    thread = Thread.new do
      mesg, from = peer.recvfrom(16)
      peer.send(mesg.upcase, 0, from[3], from[1])
    end
    yield peer.addr[1]
  ensure
    thread&.kill
    peer&.close
  end

  # A Sandbox whose one UDP endpoint reaches loopback +port+.
  def sandbox_sending_to(port)
    Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM).network(:local) do |net|
      net.endpoint "127.0.0.1", port: port, protocol: :udp, addresses: [:local]
    end
  end

  # A loopback server that resets its connection once it reads a line.
  def with_resetting_server
    server = TCPServer.new("127.0.0.1", 0)
    thread = Thread.new { reset_once(server) }
    yield server.addr[1]
  ensure
    thread&.join(10)
    server&.close
  end

  def reset_once(server)
    client = server.accept
    client.gets
    client.setsockopt(Socket::SOL_SOCKET, Socket::SO_LINGER, [1, 0].pack("ii"))
    client.close
  end

  def answer_once(server, response)
    client = server.accept
    read_head(client)
    client.write(response)
    client.close
  end

  # The request line and headers, up to the blank line.
  def read_head(client)
    head = []
    while (line = client.gets) && line != "\r\n"
      head << line.chomp
    end
    head
  end

  # A port other than +port+, within the 16 bits a port holds whichever port
  # the server was given.
  def port_besides(port)
    port ^ 1
  end

  def sandbox_reaching(port, **options)
    Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM, **options).network(:local) do |net|
      net.endpoint "127.0.0.1", port: port, protocol: :tcp, addresses: [:local]
    end
  end
end
