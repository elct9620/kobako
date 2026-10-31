# Security Model

kobako isolates untrusted guest code; it does not decide what that code may reach.
The first job is the gem's, the second is yours, and this guide draws the line.
The behavior itself is specified in [`spec/behavior/`](../spec/behavior/).

```
   kobako owns                          you own
   ───────────                          ───────
   the isolation boundary               which Services and Mounts cross it
   resource caps                        what each Service may do
   wire / return-value guardrails       input validation on Service args
   per-invocation + cross-Sandbox       one Sandbox per trust context
     isolation                          where preloaded bytecode came from
```

The guest reaches outward only through a Service you bound or a Filesystem you
mounted. Guest code can name any `MyService::KV` path, but a forged name resolves
only to something you bound. **What you bind and mount, and each object's Exposure,
is the real authorization gate.**

## Built-in Guarantees

These hold without host effort, so do not re-implement them. Each row names the
feature that specifies it.

| Guarantee | Specified in |
|---|---|
| a bound object exposes only what it defines itself | [transport-boundary](../spec/behavior/transport-boundary.md) |
| reflection, eval, and methods mixed into every object never cross, whatever an object permits | [transport-boundary](../spec/behavior/transport-boundary.md) |
| the guest cannot forge or dereference a Handle | [transport-boundary](../spec/behavior/transport-boundary.md) |
| each invocation starts from the same boot state | [sandbox](../spec/behavior/sandbox.md) |
| a binding belongs to its own Sandbox | [services](../spec/behavior/services.md) |
| no ambient filesystem, environment, or socket | [runtime](../spec/behavior/runtime.md) |
| a path the guest gives stays within its Mount, and the Mount's access holds | [runtime](../spec/behavior/runtime.md) |
| no live time or entropy under the default `:hermetic` | [runtime](../spec/behavior/runtime.md) |
| timeout, memory cap, and output clipping fail cleanly | [sandbox](../spec/behavior/sandbox.md) |
| the guest holds no more files open than its cap | [sandbox](../spec/behavior/sandbox.md) |
| a Tmpfs holds no more than its size | [filesystem](../spec/behavior/filesystem.md) |
| a value the wire cannot carry becomes a Handle or a controlled error | [transport-dispatch](../spec/behavior/transport-dispatch.md) |

## Isolation Profiles

Postures form the ladder `permissive < hermetic`; `Sandbox.new(profile:)` requests
one and defaults to `:hermetic`. The rungs differ in what the guest reads of time, entropy, and a Mount's entries.

| Profile | Clocks and entropy | A Mount's entries | Everything else |
|---|---|---|---|
| `hermetic` | frozen clock, constant entropy | every time at the epoch, inode number 0 | no ambient filesystem, `ENV`, or socket |
| `permissive` | live host time and entropy | real times, a keyed inode number | same as `hermetic` |

A permissive guest's inode number is a hash of the entry's identity under a key the process draws once. It tells entries apart, and nothing outside the process can compute it from a host's numbers.

`hermetic` keeps the guest deterministic except for values a Service injects.
Requesting `:permissive` trades that reproducibility for real time and entropy;
every other guarantee holds on both rungs.

The request is also a floor. Keep the default when the engine is swappable. An
engine that cannot deny ambient authority is then refused at construction, not
silently weakening these guarantees. The ladder is specified in
[runtime](../spec/behavior/runtime.md).

## Uncapped Limits

Passing `nil` turns a cap off, and with it the bound that cap kept. Each one then
leaves the host exposed in its own way.

| `nil` cap | What the guest can then cost the host |
|---|---|
| `timeout` | a thread held for as long as the guest runs |
| `setup_timeout` | `Sandbox.new` held for as long as the Guest Binary takes to start |
| `memory_limit` | up to 4 GiB of guest memory, and no value budget |
| `stdout_limit`, `stderr_limit` | host memory without bound, read back after the run ends |
| `open_file_limit` | host descriptors for each file or directory open beneath a Bind |
| a Tmpfs's `size:` | host memory without bound; one write far past a file's end fills the gap |
| a Tmpfs's `nr_inodes:` | an entry for every file and directory it makes |

An uncapped output is the sharpest: a guest writing one buffer in a loop grows the
capture on the host, not in its own memory. Reading that capture back also runs
after the timeout fires, so the call returns late. Keep both output caps set for
untrusted code.

## Preloaded Bytecode

`preload(binary:)` runs its blob as guest code without compiling it. mruby checks
the blob's structure, not its instructions, so trust a blob as you trust host
code. A blob outside that trust stays inside the Wasm sandbox. It can corrupt only
the guest's own memory, which the host already treats as hostile.

| Blob | Pass it to `preload(binary:)` |
|---|---|
| unmodified compiler output of the Guest Binary's mruby release | yes |
| anything else, such as an upload or a hand-edited dump | no; preload the source instead |

## Service Design

A Service is the one place untrusted code touches your application, so each
binding is a capability you hand out. Ask these questions before you bind one.

| Concern | Lever |
|---|---|
| Trust Context | one Sandbox per principal |
| Method Surface | bind a purpose-built object |
| Self-gating Objects | a private `respond_to_guest?` |
| Input Validation | reject at method entry |
| External Effects | allowlist the resolved resource |
| Return Surface | return terminal values |
| Work Volume | budget each invocation |

### Trust Context

A Sandbox's bindings *are* its capability set, so one Sandbox shared across contexts
turns every binding into ambient authority for all of them. Build one per principal
— per user, agent session, or submission — and bind only what that context may
touch. Finish every `bind`, `preload`, and `flag` before the first invocation, which seals
the registry.

```ruby
def sandbox_for(session)
  Kobako::Sandbox.new.tap do |s|
    s.bind("KV::Store", ScopedStore.new(session.id))  # only this session's keys
  end
end
```

### Method Surface

`bind` exposes every public method the object's own class and the object itself
define, not just the one you had in mind. What it inherits or mixes in stays out, as
[transport-boundary](../spec/behavior/transport-boundary.md) specifies. Bind a
purpose-built object rather than a capable one whose other methods leak more than
you intend.

```ruby
# Either binding, not both: one path takes one object.
sandbox.bind("Cfg::Settings", AppConfig.current)  # reachable: secret_key, database_url, writers, ...

class ThemeReader
  def color = AppConfig.current.theme.color
end
sandbox.bind("Cfg::Settings", ThemeReader.new)    # reachable: only #color
```

#### Surface Gotchas

Two cases surprise hosts that follow the rule above. kobako cannot tell your
classes from a gem's or the standard library's, and reflective names never cross.

| Case | What the guest meets | Do instead |
|---|---|---|
| a class you did not write, such as `Pathname` | its Ruby-source methods, `#rmtree` and `#mkpath` among them | bind your own class, or narrow with `respond_to_guest?` |
| a Service method named `send`, `eval`, `binding`, `method`, … | a proxy refusal, or for `send` the guest's own `Kernel#send` | rename it; never reuse names across trust layers |

### Self-gating Objects

A bound object, or anything crossing back as a `Kobako::Handle`, may define a private
`respond_to_guest?(name)` that decides which names the guest may call. Answer
`false` for every name and the object is **opaque**: the guest carries it to another
Service but reads nothing. That is the bearer-token shape a credential wants.

Answer `true` for a subset, such as `name == :public_id`, to expose exactly those.
The predicate chooses among the methods written in Ruby and the names the object
answers dynamically. A method built into Ruby stays refused, however it was reached.
So does one a library mixes into every object, such as ActiveSupport's `try` or
`to_json`; define a Service's own `to_json` to expose one. Keep the predicate
private all the same. The rules
live in [transport-boundary](../spec/behavior/transport-boundary.md).

```ruby
class ApiCredential
  def headers = { authorization: "Bearer #{token}" }   # host-side callers only

  private

  def token = Vault.fetch(:api_key)
  def respond_to_guest?(_name) = false                 # opaque: carried, never read
end

# A Service issues the credential; being non-wire it crosses as an opaque Handle.
sandbox.bind("Secret::Issue", -> { ApiCredential.new })

# guest:  cred = Secret::Issue.call            # a Handle it holds but cannot read
#         WebFetch::Get.call(url, cred: cred)  # forwards it to another Service
# host:   WebFetch receives the real ApiCredential and calls #headers
```

### Dynamic Backends

A name a `method_missing` backend answers dynamically is not one its class defines,
so the default leaves it out. Answer those names in `respond_to_missing?`, name
the callable ones in a private `respond_to_guest?`, and keep that list narrow. A predicate answering `true` for
everything sends every non-reflection name to `method_missing`.

```ruby
class StoreBackend
  def initialize(store) = @store = store
  def method_missing(name, *args) = @store.public_send(name, *args)
  def respond_to_missing?(name, include_private = false) = @store.respond_to?(name) || super

  private

  def respond_to_guest?(name) = %i[get put].include?(name)  # the whole vocabulary
end
```

### Input Validation

Every argument arrives from untrusted code. It may pass `2.5` where you expect an
Integer, a negative count, or a value large enough to exhaust memory. Reject bad
type, range, and encoding (CR/LF, NUL) at the method entry. A quiet coercion is a
host-side defect the sandbox cannot catch.

```ruby
sandbox.bind("Text::Repeat", ->(str, n) {
  raise ArgumentError, "n must be 1..100" unless n.is_a?(Integer) && (1..100).cover?(n)
  str.to_str * n
})
```

### External Effects

An allowlisted name can resolve to an internal address at use time (DNS rebinding).
A Service that reaches the network, disk, or another system should allowlist what it
permits rather than denylist what it forbids. Verify the *resolved resource*, not the
name the guest handed you, and re-check on every redirect hop.

```ruby
ALLOWED = { "api.example.com" => 443 }.freeze

sandbox.bind("Net::Get", ->(url) {
  uri = URI(url)
  ip  = Resolv.getaddress(uri.host)                       # resolve first
  raise "host not allowed" unless ALLOWED[uri.host] == uri.port && public_ip?(ip)  # then verify the IP
  Net::HTTP.get(uri)
})
```

### Return Surface

A return the wire cannot carry crosses as a `Kobako::Handle`, and its own methods
become reachable. Return the data the guest needs as a terminal value, not a host
object it can keep calling into. When the guest must hold the object itself, give it
a `respond_to_guest?` that seals or narrows that surface.

```ruby
sandbox.bind("Search::Docs", ->(q) { index.query(q).map(&:title) })  # => ["...", "..."]
sandbox.bind("Search::Hit",  ->(q) { index.query(q).first })         # => a Handle whose own methods dispatch back
```

A returned collection holding host objects crosses as one Handle too, and the
guest cannot index into it. Map it to terminal values on the host side.

The same applies to failures. An exception a Service raises reaches the guest as
`<class>: <message>` fault text, and an `ArgumentError` as its message alone. Rescue
internal errors and re-raise a clean, guest-safe one, so secrets and internal detail
stay out of the message.

### Work Volume

The timeout bounds an invocation's time, Service time included, but not the host
memory and work each call costs. A Handle costs the guest only its reference, while
its object stays in host memory until the invocation ends
([transport-dispatch](../spec/behavior/transport-dispatch.md)). For hostile input,
bound the calls and Handles a single invocation can create.

Each payload the guest sends holds at most one value per 16 bytes of `memory_limit`
([payload-msgpack](../wire/payload-msgpack.md)). Raising the memory cap raises what one
call may make the host build.

```ruby
sandbox.bind("Cur::Next")  # filled per invocation

def budgeted(cursor, limit: 1_000)
  calls = 0
  -> {
    raise "budget exhausted" if (calls += 1) > limit
    cursor.advance  # a fresh Handle each call
  }
end

sandbox.eval(script) { |ctx| ctx.bind("Cur::Next", budgeted(cursor)) }
```

A fresh budget per `ctx.bind` resets the count for every invocation. A counter
captured once at `bind` would span the Sandbox's lifetime instead.

## Mount Design

A Mount hands the guest a Filesystem at one path. kobako keeps the guest beneath
that path and holds its access; what the Filesystem holds is yours to choose. A
Bind opens only files and directories: a FIFO, socket, or device raises
`Errno::EACCES`. On Windows, so does a spelling the host lists under no name.

| Choose | Because |
|---|---|
| `access: :read` unless the guest must write | a read-only Mount refuses every change |
| a Tmpfs over a Bind | a Bind is a real host path the guest changes |
| `size:` and `nr_inodes:` sized to the task | the defaults hold 64 MiB and 16385 entries |
| `open_file_limit:` sized against the host's own | a file holds up to two host descriptors, and a directory one per Bind Layer |

### Mount Blocks

A block decides each open and each change by path, at most once for each. Allow what the guest needs
rather than refusing what it must not reach: a denylist misses a path you did not
foresee. The block judges paths, so nothing may reach a path it never judged:

| Beneath a Mount with a block | The guest sees |
|---|---|
| a rename of a directory | `Errno::EXDEV`, and it copies entry by entry |
| a path through a host link | `Errno::ELOOP` |
| a link another writer swaps in after the block judges | never followed; the call reaches the directory it walked to |
| another case or Unicode form of a name | the block judges the spelling the host stores |

A call that what stands at the path refuses, such as `Errno::ENOENT` or
`Errno::EEXIST`, fails before the block is asked.

```ruby
sandbox.mount("/data", fs) { |path, _access| path.start_with?("/data/public/") }
```

#### Refused Paths

The block does not judge `stat` or a listing, as Landlock leaves `stat(2)` alone. A refused path keeps its content private and still reports the rest.

| The guest reads | Reveals |
|---|---|
| that the path exists, and its kind | what stands there |
| its size | how long the content is |
| its inode number, on `permissive` | whether two paths are one file |
| its count of names, on either posture | hard links, including any outside the Mount |

Keep a file out of the Mount when even these must stay private.

### Shared Backends

kobako defends a Mount against the guest and against another Sandbox's guest writing to the same backend. A change from anywhere else is the host's.

| Writer | kobako's answer |
|---|---|
| the guest itself | kept beneath its Mount, its access held |
| another Sandbox's guest sharing a backend | neither guest reaches what its Mount or block refuses |
| a guest of another Mount of the same Filesystem | only that Mount's block judges it; a block guards its own Mount's paths |
| the host, or a change to a Layer outside its Overlay | no crash or deadlock; the rest is the host's |

### Host Side

Some risks stay with the host, because kobako cannot see them.

| Risk | Where it lies |
|---|---|
| another process changing a Bind's host files | the host directory |
| a link swapped in between kobako's check and its open, without a block | the host's race, never leading beyond the Bind |
| a hard link or mount point inside a Bind | the guest reaches what it leads to |
| a Bind's host disk | kobako caps no space there; the host directory's own quota does |
| a device swapped into a Bind as the guest opens a file | opened once before its refusal |
| a link in a Bind without a block | the guest reaches its target, never beyond the Bind, and never reads where it points |
| a link through a directory the guest opened | followed only within it; leading out raises `Errno::EACCES`, as a `..` leading out does |
| a link loop without a block | `Errno::ELOOP` after 40 links, as Linux answers |
| a `..` out of where a link led | `Errno::ELOOP`, as the guest never learns where it points |
| a directory the guest lists in steps | its entries held in host memory until the listing ends, once per descriptor |
| an opened directory moved by the host or another guest | still reached through that descriptor, as Linux reaches it, unless a block refuses |
| a link in an Overlay's lower Layer | read through it; a change that would copy it up raises `Errno::EXDEV` |
| a Filesystem written in Ruby | its own atomicity, and its time counts toward `timeout:` uninterrupted |
| a `\` in a name a Ruby Filesystem receives | the Filesystem's to refuse, as Windows reads it as a separator |
| a host link a Ruby Filesystem's open meets on Windows | the Filesystem's to refuse, as Windows' Ruby has no `File::NOFOLLOW` |
| a directory a Bind holds open on Windows | the host cannot move or remove it until the Bind is collected |
| one Overlay's upper shared by another, or mounted on its own | undefined, as overlayfs leaves it; no crash or deadlock |
| one Overlay mounted in invocations that run at once | a change in one directory or file waits for the other, past `timeout:` |

### Opened Directories

A directory the guest opened is held as itself, as a POSIX descriptor holds one.
What happens to it later decides what a call through it reaches.

| After it opens | A call through it |
|---|---|
| it moves | reaches it where it went |
| it is removed | makes nothing more |

A path resolved through it stays beneath it, as a descriptor only ever narrows
what the guest may reach.

#### Beneath a Block

Each call first asks the directory's Filesystem whether the directory still
stands where the runtime last saw it. The block then judges where it stands now.

| How it moved | A call through it |
|---|---|
| through any Mount of the same Filesystem | reaches it where it went, judged there |
| by the host, or through another Filesystem | raises `Errno::ENOENT` |

A move the Filesystem never saw leaves another directory there, or none. The
Filesystem compares what stands there with the identity the directory opened
with, and refuses it. A Bind holds the host directory open meanwhile, so the
host never hands its inode number to the next one made. The comparison and the
call are two steps, so a swap between them is the host's race.
On Windows a Bind reports no identity. It refuses a
path holding a backslash there, so a block never judges one name while the host
walks two.
