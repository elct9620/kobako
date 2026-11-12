# Network interface

What a Host App writes against to give the Guest the network. Each named entry
is one Grant, and its endpoints are data the runtime judges on any thread.

## Includes

- `crates/kobako-runtime/src/network.rs`
- `crates/kobako-runtime/src/network/builder.rs`

## `Entry::endpoints`

What the Guest may reach through the entry; one playing no such role reaches
nothing.

```rust
pub trait Entry {
    fn endpoints(&self) -> Vec<Endpoint>;
}
```

## `Entry::policies`

The hooks the entry's policies answer; one it leaves out has no opinion.

```rust
pub trait Entry {
    fn policies(&self) -> Policies;
}
```

## `Entry::credentials`

The credentials swapped into a request bound to the entry's own endpoints.

```rust
pub trait Entry {
    fn credentials(&self) -> Vec<Credential>;
}
```

## `Policies`

An entry's policies, one optional hook per decision point. A missing hook has
no opinion, and a policy only narrows.

```rust
pub struct Policies {
    pub resolve: Option<Arc<dyn for<'a> Policy<Name<'a>>>>,
    pub connect: Option<Arc<dyn for<'a> Policy<Connection<'a>>>>,
    pub request: Option<Arc<dyn for<'a> Policy<Request<'a>>>>,
}
```

## `Policies::with_request`

These policies with one judging each HTTP request.

```rust
impl Policies {
    pub fn with_request(self, policy: Arc<dyn for<'a> Policy<Request<'a>>>) -> Self {}
}
```

## `Request`

One HTTP request the Guest sends: the authority, method, scheme, path with no
query, and header names. It never holds a header's value.

```rust
pub struct Request<'a> {
    pub authority: (&'a str, u16),
    pub method: &'a str,
    pub scheme: &'a str,
    pub path: &'a str,
    pub header_names: &'a [&'a str],
}
```

## `Request::new`

The request to an authority by a method, scheme, path, and header names.

```rust
impl<'a> Request<'a> {
    pub fn new(authority: (&'a str, u16), method: &'a str, scheme: &'a str, path: &'a str, header_names: &'a [&'a str]) -> Self {}
}
```

## `Builder::request_policy`

Judge each HTTP request with one more policy; any refusal wins.

```rust
impl Builder {
    pub fn request_policy(&mut self, policy: impl for<'a> Policy<Request<'a>> + 'static) -> &mut Self {}
}
```

## `Policies::with_resolve`

These policies with one judging each lookup.

```rust
impl Policies {
    pub fn with_resolve(self, policy: Arc<dyn for<'a> Policy<Name<'a>>>) -> Self {}
}
```

## `Name`

One name the Guest asks to look up, lowercase and with no trailing dot, an IDN
as its A-label.

```rust
pub struct Name<'a> {
    pub name: &'a str,
}
```

## `Name::new`

The name to look up.

```rust
impl<'a> Name<'a> {
    pub fn new(name: &'a str) -> Self {}
}
```

## `Policies::with_connect`

These policies with one judging each connect.

```rust
impl Policies {
    pub fn with_connect(self, policy: Arc<dyn for<'a> Policy<Connection<'a>>>) -> Self {}
}
```

## `Builder`

An entry built in code, as Ruby's DSL block builds one. Its endpoints,
credentials, and policies add to those of an entry given beside it.
`Builder::default()` adds to none, as a block given alone does.

```rust
pub struct Builder {}
```

## `Builder::new`

A builder adding to an entry.

```rust
impl Builder {
    pub fn new(entry: Arc<dyn Entry>) -> Self {}
}
```

## `Builder::endpoint`

Let the Guest reach one more endpoint.

```rust
impl Builder {
    pub fn endpoint(&mut self, endpoint: Endpoint) -> &mut Self {}
}
```

## `Builder::resolve_policy`

Judge each lookup with one more policy; any refusal wins.

```rust
impl Builder {
    pub fn resolve_policy(&mut self, policy: impl for<'a> Policy<Name<'a>> + 'static) -> &mut Self {}
}
```

## `Builder::connect_policy`

Judge each connect with one more policy; any refusal wins.

```rust
impl Builder {
    pub fn connect_policy(&mut self, policy: impl for<'a> Policy<Connection<'a>> + 'static) -> &mut Self {}
}
```

## `Connection`

One connection the Guest asks for: the host it named, its port, and its
protocol. The host is never a real address a name resolved to.

```rust
pub struct Connection<'a> {
    pub host: &'a str,
    pub port: u16,
    pub protocol: Protocol,
}
```

## `Connection::new`

The connection to a host, a port, and a protocol.

```rust
impl<'a> Connection<'a> {
    pub fn new(host: &'a str, port: u16, protocol: Protocol) -> Self {}
}
```

## `Endpoint`

What the Guest may reach: a host, a port, a protocol, and where the host's name
may lead.

```rust
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    pub addresses: Vec<Addresses>,
    pub rules: Vec<HttpRule>,
}
```

## `Endpoint::with_rules`

This endpoint judging each request by the rules given.

```rust
impl Endpoint {
    pub fn with_rules(self, rules: Vec<HttpRule>) -> Self {}
}
```

## `HttpRule`

One rule an `http` endpoint judges a request by: a method or any, and a path
pattern. `*` stands for one segment, and `**` for any number.

```rust
pub struct HttpRule {
    pub action: Action,
    pub method: Option<String>,
    pub path: String,
}
```

## `HttpRule::new`

A rule taking an action on a method, or any, on the paths a pattern matches.

```rust
impl HttpRule {
    pub fn new(action: Action, method: Option<&str>, path: impl Into<String>) -> Self {}
}
```

## `Action`

What an `HttpRule` does with a request it matches: allow it or deny it.

```rust
pub enum Action {
    Allow,
    Deny,
}
```

## `Endpoint::new`

An endpoint speaking HTTP to public addresses, refused for an empty host or
port zero.

```rust
impl Endpoint {
    pub fn new(host: impl Into<String>, port: u16) -> Result<Self, EntryError> {}
}
```

## `Endpoint::with_protocol`

The endpoint speaking another protocol.

```rust
impl Endpoint {
    pub fn with_protocol(self, protocol: Protocol) -> Self {}
}
```

## `Endpoint::with_addresses`

The endpoint leading to other addresses.

```rust
impl Endpoint {
    pub fn with_addresses(self, addresses: Vec<Addresses>) -> Self {}
}
```

## `Protocol`

What an endpoint speaks: HTTP, TCP, or UDP.

```rust
pub enum Protocol {}
```

## `Addresses`

Where an endpoint's name may lead: an address set, or a CIDR.

```rust
pub enum Addresses {}
```

## `EntryError`

An entry or an endpoint no connection could follow.

```rust
pub struct EntryError {}
```

## `Secret`

A value the Host App hands no Guest. Its `Debug` prints a mask, and its bytes
are zeroed as it drops.

```rust
pub struct Secret {}
```

## `Secret::new`

A secret holding these bytes.

```rust
impl Secret {
    pub fn new(value: impl Into<Vec<u8>>) -> Self {}
}
```

## `Placement`

Where a credential's value goes: the `authorization` header as a bearer token
or a basic pair, or one named header.

```rust
pub enum Placement {
    Bearer,
    Basic,
    Header(String),
}
```

## `CredentialSource`

What resolves a credential's value as a bound request leaves. A closure fits.

```rust
pub trait CredentialSource: Send + Sync {}
```

## `CredentialSource::value_of`

The value for a request, or `None` to send nothing.

```rust
pub trait CredentialSource {
    fn value_of(&self, request: &Request<'_>) -> Option<Secret>;
}
```

## `Credential`

A placement and a placeholder paired with the source of the value that
replaces the placeholder there.

```rust
pub struct Credential {
    pub placement: Placement,
    pub placeholder: String,
    pub source: Arc<dyn CredentialSource>,
}
```

## `Credential::new`

The credential of a placement, a placeholder, and a source.

```rust
impl Credential {
    pub fn new(placement: Placement, placeholder: impl Into<String>, source: Arc<dyn CredentialSource>) -> Self {}
}
```

## `Builder::credential`

Swap one more credential into requests bound to this entry's endpoints.

```rust
impl Builder {
    pub fn credential(&mut self, credential: Credential) -> &mut Self {}
}
```

