//! Integration coverage for giving the guest the network: each named entry
//! is one Grant, registered and replaced by its name as a Mount is by its path.

use std::sync::{Arc, Mutex};

use kobako::filesystem::{Filesystem, Tmpfs};
use kobako::network::{Endpoint, Entry};
use kobako::{Access, Error, Profile, RunPayload, Sandbox};
use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::error::InvokeError;
use kobako_runtime::grant::{Grant, GrantKind};
use kobako_runtime::runtime::{self, Runtime};
use kobako_runtime::snapshot::{Capture, Completion, Snapshot, Usage};
use kobako_transport::envelope::{Outcome, Preamble};

/// Each invocation's network entries, as each name and its endpoints' hosts.
type ReceivedEntries = Arc<Mutex<Vec<Vec<(String, Vec<String>)>>>>;

/// A runtime that presents Mounts and the network, recording each network
/// entry an invocation receives.
#[derive(Default)]
struct RecordsNetwork {
    received: ReceivedEntries,
}

impl Runtime for RecordsNetwork {
    fn invoke(
        &self,
        _entry: runtime::Entry<'_>,
        _preamble: &Preamble,
        _handler: Option<Arc<dyn DispatchHandler>>,
        grants: &[Grant],
    ) -> Result<Snapshot, InvokeError> {
        let entries = grants
            .iter()
            .filter_map(|grant| match grant {
                Grant::Network(network) => Some((
                    network.name().to_string(),
                    network.endpoints().into_iter().map(|e| e.host).collect(),
                )),
                _ => None,
            })
            .collect();
        self.received.lock().unwrap().push(entries);
        Ok(Snapshot::new(
            Completion::Outcome(Outcome::Ok(vec![0xc0])),
            Capture::default(),
            Capture::default(),
            Usage::default(),
        ))
    }

    fn profile(&self) -> Profile {
        Profile::Hermetic
    }

    fn supports(&self, kind: GrantKind) -> bool {
        matches!(kind, GrantKind::Mount | GrantKind::Network)
    }
}

/// A runtime that presents no network.
struct LacksNetwork;

impl Runtime for LacksNetwork {
    fn invoke(
        &self,
        _entry: runtime::Entry<'_>,
        _preamble: &Preamble,
        _handler: Option<Arc<dyn DispatchHandler>>,
        _grants: &[Grant],
    ) -> Result<Snapshot, InvokeError> {
        unreachable!("a network entry is refused before any invocation")
    }

    fn profile(&self) -> Profile {
        Profile::Hermetic
    }
}

/// An entry reaching one host on port 443.
struct Reaches(&'static str);

impl Entry for Reaches {
    fn endpoints(&self) -> Vec<Endpoint> {
        vec![Endpoint::new(self.0, 443).expect("a host and a port stand")]
    }
}

fn sandbox() -> (Sandbox, ReceivedEntries) {
    let runtime = RecordsNetwork::default();
    let received = runtime.received.clone();
    let sandbox = Sandbox::with_runtime(runtime, Profile::Hermetic)
        .expect("a hermetic engine meets the floor");
    (sandbox, received)
}

fn invoke(sandbox: &Sandbox) {
    sandbox
        .run("App", RunPayload::from_bytes(Vec::new()))
        .expect("the recording runtime answers every invocation");
}

// @behavior NW-001
#[test]
fn an_entry_after_the_first_invocation_is_refused() {
    let (mut sandbox, _) = sandbox();
    invoke(&sandbox);
    assert!(
        matches!(
            sandbox.network("api", Arc::new(Reaches("api.example.com"))),
            Err(Error::Sealed(_))
        ),
        "Sandbox::network after the first invocation must be refused as sealed"
    );
}

// @behavior NW-002
#[test]
fn a_name_given_twice_is_refused() {
    let (mut sandbox, _) = sandbox();
    sandbox
        .network("github", Arc::new(Reaches("api.github.com")))
        .unwrap();
    assert!(
        matches!(
            sandbox.network("github", Arc::new(Reaches("other.example.com"))),
            Err(Error::Argument(_))
        ),
        "Sandbox::network with a name already given must be refused"
    );
}

// @behavior NW-003
#[test]
fn a_name_and_a_mount_path_never_meet() {
    let (mut sandbox, received) = sandbox();
    let filesystem = Filesystem::new(Arc::new(Tmpfs::default())).expect("an empty Tmpfs stands");
    sandbox.mount("/data", filesystem, Access::Read).unwrap();
    sandbox
        .network("/data", Arc::new(Reaches("api.example.com")))
        .unwrap();
    invoke(&sandbox);
    assert_eq!(
        received.lock().unwrap()[0],
        vec![("/data".to_string(), vec!["api.example.com".to_string()])],
        "Sandbox::network named as a mounted path must stand beside the Mount"
    );
}

// @behavior NW-004
#[test]
fn a_runtime_presenting_no_network_refuses_the_entry() {
    let mut sandbox = Sandbox::with_runtime(LacksNetwork, Profile::Hermetic).unwrap();
    assert!(
        matches!(
            sandbox.network("api", Arc::new(Reaches("api.example.com"))),
            Err(Error::Setup(_))
        ),
        "Sandbox::network over a runtime that supports no network must be refused as a setup \
         failure"
    );
}

// @behavior NW-005
#[test]
fn a_context_cannot_replace_a_name_never_declared() {
    let (sandbox, received) = sandbox();
    let result = sandbox.run_with("App", RunPayload::from_bytes(Vec::new()), |ctx| {
        ctx.network("other", Arc::new(Reaches("api.example.com")))
    });
    assert!(
        matches!(result, Err(Error::Argument(_))) && received.lock().unwrap().is_empty(),
        "Context::network of a name the Sandbox never gave must be refused before the guest \
         runs"
    );
}

// @behavior NW-006
#[test]
fn a_context_replaces_an_entry_for_its_invocation_only() {
    let (mut sandbox, received) = sandbox();
    sandbox
        .network("api", Arc::new(Reaches("api.example.com")))
        .unwrap();
    sandbox
        .run_with("App", RunPayload::from_bytes(Vec::new()), |ctx| {
            ctx.network("api", Arc::new(Reaches("staging.example.com")))
        })
        .unwrap();
    invoke(&sandbox);
    let hosts = |run: &Vec<(String, Vec<String>)>| run[0].1.clone();
    let received = received.lock().unwrap();
    assert_eq!(
        (hosts(&received[0]), hosts(&received[1])),
        (
            vec!["staging.example.com".to_string()],
            vec!["api.example.com".to_string()]
        ),
        "Context::network must hand its invocation the replacement and the next one the \
         Sandbox's own entry"
    );
}

// @behavior NW-111
#[test]
fn a_context_replaces_an_entry_with_what_a_builder_adds() {
    let (mut sandbox, received) = sandbox();
    sandbox
        .network("api", Arc::new(Reaches("api.example.com")))
        .unwrap();
    sandbox
        .run_with("App", RunPayload::from_bytes(Vec::new()), |ctx| {
            ctx.network_with("api", Arc::new(Reaches("staging.example.com")), |net| {
                net.endpoint(
                    Endpoint::new("extra.example.com", 443).expect("a host and a port stand"),
                );
            })
        })
        .unwrap();
    assert_eq!(
        received.lock().unwrap()[0][0].1,
        vec![
            "staging.example.com".to_string(),
            "extra.example.com".to_string()
        ],
        "Context::network_with must hand its invocation the other entry's endpoints, then the \
         builder's"
    );
}

// @behavior NW-008
#[test]
fn an_entry_with_an_empty_name_is_refused() {
    let (mut sandbox, _) = sandbox();
    assert!(
        matches!(
            sandbox.network("", Arc::new(Reaches("api.example.com"))),
            Err(Error::Argument(_))
        ),
        "Sandbox::network with an empty name must be refused"
    );
}

/// An entry reaching any name beneath `example.com`, led only to private
/// addresses.
#[cfg(feature = "wasmtime")]
struct WildcardToPrivate;

#[cfg(feature = "wasmtime")]
impl Entry for WildcardToPrivate {
    fn endpoints(&self) -> Vec<Endpoint> {
        vec![Endpoint::new("*.example.com", 443)
            .expect("a host and a port stand")
            .with_addresses(vec![kobako::network::Addresses::Private])]
    }
}

// @behavior NW-029
#[cfg(feature = "wasmtime")]
#[test]
fn a_wildcard_endpoint_leading_to_private_addresses_is_an_argument_error() {
    let Some(mut sandbox) = crate::common::real_sandbox() else {
        return;
    };
    sandbox
        .network("api", Arc::new(WildcardToPrivate))
        .expect("the entry registers; its endpoints compile per invocation");

    let err = sandbox
        .eval("1 + 1")
        .expect_err("a wildcard endpoint leading to private addresses must fail the invocation");

    assert!(
        matches!(err, Error::Argument(_)),
        "Sandbox::eval with a wildcard endpoint whose addresses name private must answer \
         Error::Argument, never a trap, got {err:?}"
    );
}

// @behavior NW-212
#[cfg(feature = "wasmtime")]
#[test]
fn a_mock_handed_through_options_answers_the_guests_names() {
    use kobako::network::{Builder, Protocol};
    use kobako::Options;
    use kobako_runtime::net::{Backends, Mock};
    use std::net::{IpAddr, SocketAddr, TcpListener};

    let public = IpAddr::from([93, 184, 216, 34]);
    let server = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let mock = Arc::new(Mock::default());
    mock.answer("app.test", vec![public]);
    mock.route(
        SocketAddr::new(public, 80),
        server.local_addr().expect("the server's address"),
    );
    let mut options = Options::default();
    options.network = Backends::new(mock.clone(), mock.clone());
    let Some(mut sandbox) = crate::common::real_sandbox_with(options) else {
        return;
    };
    let mut entry = Builder::default();
    entry.endpoint(
        Endpoint::new("app.test", 80)
            .expect("a host and a port stand")
            .with_protocol(Protocol::Tcp),
    );
    sandbox
        .network("web", Arc::new(entry))
        .expect("the entry registers");

    let execution = sandbox
        .eval(r#"TCPSocket.new("app.test", 80).close; print :connected"#)
        .expect("the invocation runs");

    assert_eq!(
        (execution.stdout(), mock.dials()),
        (&b"connected"[..], vec![SocketAddr::new(public, 80)]),
        "a Sandbox built with Options::network as a Mock must answer the guest's name from \
         the Mock and dial its route, got stdout {:?}",
        String::from_utf8_lossy(execution.stdout())
    );
}
