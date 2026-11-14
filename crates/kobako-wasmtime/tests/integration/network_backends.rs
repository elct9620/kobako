//! A Host App's own network backends, handed to the Driver's config, beneath
//! the rules the invocation's entries make.

use std::io;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use kobako_runtime::grant::Grant;
use kobako_runtime::net::{Backends, Mock, ProtocolFamily, Resolver};
use kobako_runtime::network::{Builder, Endpoint, Network, Protocol};
use kobako_runtime::snapshot::Completion;
use kobako_runtime::{Entry, Errno, Runtime};
use kobako_transport::envelope::{Outcome, Preamble};

use crate::support;

/// A public address, which an endpoint naming a host lets through.
const PUBLIC: [u8; 4] = [93, 184, 216, 34];

/// Another public address, which no route leads anywhere.
const OTHER_PUBLIC: [u8; 4] = [93, 184, 216, 35];

/// A family refusing every dial and a resolver answering two names, each
/// recording what it was asked.
#[derive(Default)]
struct Recorded {
    dialed: Mutex<Vec<SocketAddr>>,
    resolved: Mutex<Vec<String>>,
}

impl ProtocolFamily for Recorded {
    fn connect(&self, addr: SocketAddr) -> io::Result<TcpStream> {
        self.dialed.lock().unwrap().push(addr);
        Err(io::ErrorKind::ConnectionRefused.into())
    }

    fn local_addresses(&self) -> io::Result<Vec<IpAddr>> {
        Ok(Vec::new())
    }
}

impl Resolver for Recorded {
    fn resolve(&self, name: &str) -> io::Result<Vec<IpAddr>> {
        self.resolved.lock().unwrap().push(name.to_string());
        Ok(vec![match name {
            "app.test" => IpAddr::from(PUBLIC),
            _ => IpAddr::from([169, 254, 169, 254]),
        }])
    }
}

fn entry() -> Grant {
    let mut entry = Builder::default();
    for host in ["app.test", "meta.test"] {
        entry.endpoint(
            Endpoint::new(host, 80)
                .unwrap()
                .with_protocol(Protocol::Tcp),
        );
    }
    Grant::Network(Network::new("web", Arc::new(entry)).unwrap())
}

/// Run `script` on a driver whose network is `mock`, under one `tcp` entry
/// naming each of `hosts` at port 80, answering what it printed.
fn print_through(mock: &Arc<Mock>, hosts: &[&str], script: &str) -> String {
    let backends = Backends::new(mock.clone(), mock.clone());
    let driver = support::bundled_driver_with(|config| config.network = backends)
        .expect("the Guest Binary must be built");
    let mut entry = Builder::default();
    for &host in hosts {
        entry.endpoint(
            Endpoint::new(host, 80)
                .unwrap()
                .with_protocol(Protocol::Tcp),
        );
    }
    let grant = Grant::Network(Network::new("web", Arc::new(entry)).unwrap());
    let snapshot = driver
        .invoke(
            Entry::Eval {
                source: script.as_bytes(),
            },
            &Preamble::default(),
            None,
            &[grant],
        )
        .expect("the invocation must start");
    let Completion::Outcome(Outcome::Ok(_)) = snapshot.completion else {
        panic!("the invocation must answer rather than trap or raise");
    };
    String::from_utf8(snapshot.stdout.bytes).unwrap()
}

/// The guest opening a TCPSocket to each name, answering `:connected` or
/// the class of what it raised.
const CONNECT_EACH: &str = r#"
    print ARGV_HOSTS.map { |host|
      begin
        TCPSocket.new(host, 80).close
        :connected
      rescue => e
        e.class.to_s
      end
    }.inspect
"#;

fn connect_each(mock: &Arc<Mock>, hosts: &[&str]) -> String {
    let script = CONNECT_EACH.replace("ARGV_HOSTS", &format!("{hosts:?}"));
    print_through(mock, hosts, &script)
}

// @behavior NW-212
#[test]
fn a_mock_answers_names_from_its_table_and_dials_only_its_routes() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    // A connection the guest made waits in the backlog, so a missing one
    // reads as no connection rather than a wait without end.
    server.set_nonblocking(true).unwrap();
    let mock = Arc::new(Mock::default());
    mock.answer("app.test", vec![IpAddr::from(PUBLIC)]);
    mock.answer("unrouted.test", vec![IpAddr::from(OTHER_PUBLIC)]);
    mock.route(SocketAddr::from((PUBLIC, 80)), server.local_addr().unwrap());
    if support::bundled_driver().is_none() {
        return;
    }
    assert_eq!(
        (
            connect_each(&mock, &["app.test", "unrouted.test"]),
            mock.lookups(),
            mock.dials(),
            server.accept().is_ok(),
        ),
        (
            r#"[:connected, "Errno::ECONNREFUSED"]"#.to_string(),
            vec!["app.test".to_string(), "unrouted.test".to_string()],
            vec![
                SocketAddr::from((PUBLIC, 80)),
                SocketAddr::from((OTHER_PUBLIC, 80))
            ],
            true,
        ),
        "a Mock must answer each name from its table, dial a routed address's server, refuse \
         an address it routes nowhere, and read back each name and address it was asked"
    );
}

// @behavior NW-213
#[test]
fn a_mock_refuses_a_dial_with_the_error_it_was_given() {
    let mock = Arc::new(Mock::default());
    mock.answer("app.test", vec![IpAddr::from(PUBLIC)]);
    mock.refuse(SocketAddr::from((PUBLIC, 80)), Errno::ConnectionReset);
    if support::bundled_driver().is_none() {
        return;
    }
    assert_eq!(
        connect_each(&mock, &["app.test"]),
        r#"["Errno::ECONNRESET"]"#,
        "a Mock told to refuse an address with ECONNRESET must fail the guest's connect as a \
         host socket reset does"
    );
}

// @behavior NW-214
#[test]
fn a_mocks_held_name_answers_once_its_hold_ends() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let mock = Arc::new(Mock::default());
    mock.answer("held.test", vec![IpAddr::from(PUBLIC)]);
    mock.route(SocketAddr::from((PUBLIC, 80)), server.local_addr().unwrap());
    if support::bundled_driver().is_none() {
        return;
    }
    let hold = mock.hold("held.test");
    let (done, finished) = mpsc::channel();
    let running = mock.clone();
    thread::spawn(move || {
        let _ = done.send(connect_each(&running, &["held.test"]));
    });
    let asked = wait_until(|| !mock.lookups().is_empty());
    let waited = finished.recv_timeout(Duration::from_millis(200)).is_err();
    drop(hold);
    let answered = finished.recv_timeout(Duration::from_secs(10)).ok();
    assert_eq!(
        (asked, waited, answered.as_deref()),
        (true, true, Some("[:connected]")),
        "a lookup of a name a Mock holds must wait while the hold lasts, then connect once the \
         hold drops"
    );
}

/// Whether `ready` holds within ten seconds, asked every ten milliseconds.
fn wait_until(ready: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if ready() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

// @behavior NW-211
#[test]
fn a_host_apps_own_backends_dial_and_resolve_beneath_the_rules() {
    let recorded = Arc::new(Recorded::default());
    let backends = Backends::new(recorded.clone(), recorded.clone());
    let Some(driver) = support::bundled_driver_with(|config| config.network = backends) else {
        return;
    };
    let script = r#"
        print ["app.test", "meta.test"].map { |host|
          begin
            TCPSocket.new(host, 80)
          rescue => e
            e.class.to_s
          end
        }.inspect
    "#;
    let snapshot = driver
        .invoke(
            Entry::Eval {
                source: script.as_bytes(),
            },
            &Preamble::default(),
            None,
            &[entry()],
        )
        .expect("the invocation must start");
    let Completion::Outcome(Outcome::Ok(_)) = snapshot.completion else {
        panic!("the invocation must answer rather than trap or raise");
    };
    assert_eq!(
        (
            String::from_utf8(snapshot.stdout.bytes).unwrap(),
            recorded.resolved.lock().unwrap().clone(),
            recorded.dialed.lock().unwrap().clone(),
        ),
        (
            r#"["Errno::ECONNREFUSED", "Errno::EACCES"]"#.to_string(),
            vec!["app.test".to_string(), "meta.test".to_string()],
            vec![SocketAddr::from((PUBLIC, 80))],
        ),
        "a Host App's own Resolver must answer each name the guest looks up, and its own Protocol \
         Family must dial only the address the rules admit, never the metadata address they refuse"
    );
}
