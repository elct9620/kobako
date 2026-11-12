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

  # A Sandbox whose one entry reaches `localhost` at +port+ over +protocol+,
  # its resolve policy +policy+ when given.
  def localhost_sandbox(port, protocol: :tcp, &policy)
    Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM).network(:local) do |net|
      net.endpoint "localhost", port: port, protocol: protocol, addresses: [:local]
      net.policy(:resolve, &policy) if policy
    end
  end

  # Add a TCP endpoint at +port+ on loopback to the entry +net+ builds.
  def add_loopback_tcp(net, port)
    net.endpoint "127.0.0.1", port: port, protocol: :tcp, addresses: [:local]
  end

  # The addresses the host's resolver answers for `localhost`, in its order,
  # asked as the runtime asks it, with a trailing dot. A lookup in the guest
  # hands out one virtual address for each, from the bottom of 198.18.0.0/15.
  def localhost_answers
    Addrinfo.getaddrinfo("localhost.", nil, nil, :STREAM).map(&:ip_address).uniq
  end

  # A Sandbox whose one entry reaches `localhost` at +port+ over +protocol+,
  # leading only to 127.0.0.1, so each address the host answers before it is
  # refused. Fails unless the host answers another address first, since only
  # then does a guest have an address to try past.
  def localhost_ipv4_only_sandbox(port, protocol:)
    unless localhost_answers.index("127.0.0.1").to_i.positive?
      flunk "the host's resolver must answer another address for localhost before 127.0.0.1, " \
            "got #{localhost_answers.inspect}"
    end
    Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM).network(:local) do |net|
      net.endpoint "localhost", port: port, protocol: protocol, addresses: ["127.0.0.1/32"]
    end
  end

  # The virtual address a guest's lookup of `localhost` hands out for +real+.
  def localhost_virtual_address(real)
    index = localhost_answers.index(real) or
      flunk "the host's resolver answers #{localhost_answers.inspect} for localhost, without #{real}"
    "198.18.0.#{index}"
  end

  # A port other than +port+, within the 16 bits a port holds whichever port
  # the server was given.
  def other_port(port)
    port ^ 1
  end
end
