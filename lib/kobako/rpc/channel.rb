# frozen_string_literal: true

require_relative "dispatcher"
require_relative "../catalog/binding"

module Kobako
  module RPC
    # Kobako::RPC::Channel — the host-side RPC connection.
    #
    # Composes the namespace registry (+Kobako::RPC::Server+), the wasm
    # runtime (+Kobako::Wasm::Instance+), and the capability allocator
    # (+Kobako::HandleTable+) into a single object that mediates
    # host↔guest traffic. Analogous to a socket connection in a
    # traditional client/server RPC pair: the Server (namespace
    # registry) and the Instance (transport) are independent objects;
    # the Channel is what couples them at composition time.
    #
    # Two entry points span the two directions of the wire:
    #
    #   * +#dispatch(bytes)+ — Guest → Host RPC. The Wasm ext invokes
    #     this from inside +__kobako_dispatch+
    #     ({docs/behavior.md B-12}[link:../../../docs/behavior.md]).
    #   * +#yield_to_block(bytes)+ — Host → Guest re-entry into a guest
    #     block ({docs/behavior.md B-24}[link:../../../docs/behavior.md]).
    #     Used by the dispatcher's block proxy (S5+) when a Service
    #     method invokes +yield+.
    #
    # The Channel is constructed by +Kobako::Sandbox+ after both the
    # Server and the Instance exist; the Sandbox then hands the Channel
    # to the Instance via +Instance#channel=+ so the Wasm ext callback
    # routes incoming RPC through it.
    class Channel
      def initialize(server:, instance:, handler:)
        @server = server
        @instance = instance
        @handler = handler
      end

      # Guest → Host dispatch. Decodes the Request, resolves the target
      # via the Server's namespace registry (or the Catalog::Handler for
      # Capability Handles), invokes the method, and returns the
      # encoded Response bytes. Never raises — every failure path is
      # reified as a +Response.error+ envelope so the guest observes a
      # normal dispatch error rather than a wasm trap
      # ({docs/behavior.md B-12}[link:../../../docs/behavior.md]).
      def dispatch(request_bytes)
        Dispatcher.dispatch(request_bytes, @server, @handler, self)
      end

      # Host → Guest re-entry. Serialises +args_bytes+ into the active
      # wasm Instance, invokes +__kobako_yield_to_block+, and returns
      # the YieldResponse bytes the guest produced. Raises
      # +Kobako::Wasm::Error+ when called outside an active dispatch
      # frame (no ACTIVE_CALLER set on this thread).
      def yield_to_block(args_bytes)
        @instance.yield_to_block(args_bytes)
      end
    end
  end
end
