# frozen_string_literal: true

# The Sandboxes the network journeys under test/e2e/ run their guests in, each
# reaching a loopback server by the one entry it declares.
module NetworkSandboxes
  # A Sandbox whose one entry reaches each of +ports+ on loopback over TCP.
  def loopback_sandbox(*ports, **options)
    Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM, **options).network(:local) do |net|
      ports.each { |port| add_loopback_tcp(net, port) }
    end
  end

  # A Sandbox whose one entry reaches +port+ on loopback over HTTP.
  def http_sandbox(port)
    Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM).network(:web) do |net|
      net.endpoint "127.0.0.1", port: port
    end
  end

  # A Sandbox whose one entry reaches `localhost` at +port+ over TCP, its
  # resolve policy +policy+ when given.
  def localhost_sandbox(port, &policy)
    Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM).network(:local) do |net|
      net.endpoint "localhost", port: port, protocol: :tcp, addresses: [:local]
      net.policy(:resolve, &policy) if policy
    end
  end

  # Add a TCP endpoint at +port+ on loopback to the entry +net+ builds.
  def add_loopback_tcp(net, port)
    net.endpoint "127.0.0.1", port: port, protocol: :tcp, addresses: [:local]
  end

  # A port other than +port+, within the 16 bits a port holds whichever port
  # the server was given.
  def other_port(port)
    port ^ 1
  end
end
