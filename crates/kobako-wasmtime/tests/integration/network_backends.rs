//! A Host App's own network backends, handed to the Driver's config, beneath
//! the rules the invocation's entries make.

use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};

use kobako_runtime::grant::Grant;
use kobako_runtime::net::{Backends, ProtocolFamily, Resolver};
use kobako_runtime::network::{Builder, Endpoint, Network, Protocol};
use kobako_runtime::snapshot::Completion;
use kobako_runtime::{Entry, Runtime};
use kobako_transport::envelope::{Outcome, Preamble};

use crate::support;

/// A public address, which an endpoint naming a host lets through.
const PUBLIC: [u8; 4] = [93, 184, 216, 34];

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
