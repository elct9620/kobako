# frozen_string_literal: true

require "test_helper"

# A bound object narrows its own guest-reachable surface through the opt-in
# private predicate +respond_to_guest?+
# ({docs/behavior/security.md B-50}[link:../../docs/behavior/security.md]): falsy for every name
# is opaque, truthy for a subset is an allow-list. The predicate composes
# beneath the B-42 reflection floor and can only narrow — it never re-opens the
# +send+ / +eval+ surface the floor rejects, so the bound object never becomes
# an authority over its own security gate.
class TestDispatchGuestNarrowing < Minitest::Test
  # respond_to_guest? false for every name: holdable and passable, but the
  # guest can call nothing on it.
  class Opaque
    def token = "s3cr3t"
    def decrypt = "plain"

    private

    def respond_to_guest?(_name) = false
  end

  # respond_to_guest? truthy for a chosen subset: an allow-list.
  class AllowList
    def headers = { authorization: "Bearer x" }
    def body = "private"

    private

    def respond_to_guest?(name) = name == :headers
  end

  # respond_to_guest? permits every name; the floor must still reject the
  # reflection surface, so a bound object cannot widen past it.
  class Widener
    def safe = "ok"

    private

    def respond_to_guest?(_name) = true
  end

  # No predicate: the default full Service surface stays reachable.
  class Plain
    def hello = "hi"
  end

  # A dynamic Service resolves names through +method_missing+; its predicate
  # permits every name. respond_to_guest? answers permission, not existence,
  # so a permitted name that the dynamic dispatch cannot satisfy still *runs*
  # and fails as a runtime fault — it is never the undefined narrowing fault.
  class Dynamic
    def respond_to_missing?(_name, _include_private = false) = true

    def method_missing(name, *)
      return "dynamic:#{name}" if name == :known

      raise "no handler for #{name}"
    end

    private

    def respond_to_guest?(_name) = true
  end

  def setup
    @handler = Kobako::Catalog::Handles.new
    @services = Kobako::Catalog::Services.new(handler: @handler)
    { Cred: Opaque.new, Report: AllowList.new, Wide: Widener.new, Open: Plain.new, Dyn: Dynamic.new }
      .each { |name, service| @services.bind("Cfg::#{name}", service) }
    @services.seal!
    @yield = ->(_bytes) { raise "no block" }
  end

  def dispatch(target, method, args = [])
    req = Kobako::Transport::Request.new(target: target, method_name: method, args: args)
    bytes = Kobako::Transport::Dispatcher.dispatch(req.encode, @services, @handler, @yield)
    Kobako::Transport::Response.decode(bytes)
  end

  # The undefined fault discloses nothing about which methods the object
  # defines — opacity must not leak its surface through a distinct error.
  def test_opaque_object_rejects_every_method
    %w[token decrypt].each do |meth|
      resp = dispatch("Cfg::Cred", meth)
      assert_equal Kobako::Transport::STATUS_ERROR, resp.status,
                   "an opaque object's #{meth} through guest dispatch must be rejected, not invoked on the host"
      assert_equal "undefined", resp.payload.type,
                   "the opaque object's rejection must surface as the undefined fault (E-48)"
    end
  end

  # The headline credential path: an opaque object handed back as a Capability
  # Handle (B-14) and reached by the guest as a Handle target (B-17) is narrowed
  # by the same chokepoint as a Member — the guest holds it but calls nothing.
  def test_opaque_object_is_narrowed_through_a_handle_target
    id = @handler.alloc(Opaque.new).id
    resp = dispatch(Kobako::Handle.restore(id), "token")
    assert_equal Kobako::Transport::STATUS_ERROR, resp.status,
                 "an opaque object reached as a Handle target must be narrowed identically to a Member"
    assert_equal "undefined", resp.payload.type,
                 "the Handle-target rejection must surface as the undefined fault (E-48)"
  end

  def test_allow_list_exposes_only_the_permitted_subset
    permitted = dispatch("Cfg::Report", "headers")
    assert_equal Kobako::Transport::STATUS_OK, permitted.status,
                 "an allow-listed method through guest dispatch must stay reachable"
    assert_equal({ authorization: "Bearer x" }, permitted.payload,
                 "the permitted method through guest dispatch must return its value across the boundary")

    denied = dispatch("Cfg::Report", "body")
    assert_equal Kobako::Transport::STATUS_ERROR, denied.status,
                 "a method outside the allow-list through guest dispatch must be rejected"
    assert_equal "undefined", denied.payload.type,
                 "the non-permitted method rejection must surface as the undefined fault (E-48)"
  end

  def test_predicate_cannot_widen_past_the_reflection_floor
    rce = dispatch("Cfg::Wide", "send", [:eval, "1"])
    assert_equal Kobako::Transport::STATUS_ERROR, rce.status,
                 "send must stay rejected by the floor even when the predicate permits every name"
    assert_equal "undefined", rce.payload.type,
                 "send must be rejected by the reflection floor (E-43 undefined), not incidentally by the predicate"

    own = dispatch("Cfg::Wide", "safe")
    assert_equal Kobako::Transport::STATUS_OK, own.status,
                 "the object's own Service method must stay reachable when its predicate permits the name"
    assert_equal "ok", own.payload
  end

  # respond_to_guest? answers permission, not existence: a permitted name is
  # still subject to the method actually running. A dynamic method_missing
  # Service that cannot satisfy the name fails as a runtime fault (E-11), never
  # as the undefined narrowing fault — so a truthy predicate never disguises a
  # failed dispatch as a narrowing rejection.
  def test_permitted_dynamic_name_runs_rather_than_being_narrowed
    handled = dispatch("Cfg::Dyn", "known")
    assert_equal Kobako::Transport::STATUS_OK, handled.status,
                 "a permitted name the dynamic Service satisfies must run and return its value"
    assert_equal "dynamic:known", handled.payload,
                 "the dynamic dispatch result must cross the boundary as the Service value"

    boom = dispatch("Cfg::Dyn", "missing")
    assert_equal Kobako::Transport::STATUS_ERROR, boom.status,
                 "a permitted name the dynamic Service cannot satisfy must fail at dispatch, not be narrowed away"
    assert_equal "runtime", boom.payload.type,
                 "the failed dynamic dispatch must surface as a runtime fault (E-11), not the undefined narrowing fault"
  end

  def test_object_without_predicate_keeps_full_service_surface
    resp = dispatch("Cfg::Open", "hello")
    assert_equal Kobako::Transport::STATUS_OK, resp.status,
                 "an object without respond_to_guest? must keep its full Service surface through guest dispatch"
    assert_equal "hi", resp.payload
  end

  def test_guest_cannot_invoke_the_private_predicate_itself
    resp = dispatch("Cfg::Report", "respond_to_guest?", [:headers])
    assert_equal Kobako::Transport::STATUS_ERROR, resp.status,
                 "respond_to_guest? through guest dispatch must be unreachable, never invoked on the host"
    assert_equal "undefined", resp.payload.type,
                 "the unreachable predicate through guest dispatch must surface as the undefined fault"
  end
end
