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

  def sandbox_reaching(port, **options)
    Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM, **options).network(:local) do |net|
      net.endpoint "127.0.0.1", port: port, protocol: :tcp, addresses: [:local]
    end
  end
end
