//! An entry built in code, adding to one given beside it, as Ruby's
//! `Kobako::Network::Builder` adds a block's endpoints and policies to an object.

use std::sync::Arc;

use super::{Connection, Endpoint, Entry, Policies};
use crate::grant::Policy;

/// An entry whose endpoints and policies are the given entry's, then its own.
#[derive(Default)]
pub struct Builder {
    entry: Option<Arc<dyn Entry>>,
    endpoints: Vec<Endpoint>,
    connect: Vec<Arc<dyn for<'a> Policy<Connection<'a>>>>,
}

impl Builder {
    /// A builder adding to `entry`.
    pub fn on(entry: Arc<dyn Entry>) -> Self {
        Self {
            entry: Some(entry),
            ..Self::default()
        }
    }

    /// Let the guest reach `endpoint` too.
    pub fn endpoint(&mut self, endpoint: Endpoint) -> &mut Self {
        self.endpoints.push(endpoint);
        self
    }

    /// Judge each connect with `policy` too; any refusal wins.
    pub fn connect_policy(
        &mut self,
        policy: impl for<'a> Policy<Connection<'a>> + 'static,
    ) -> &mut Self {
        self.connect.push(Arc::new(policy));
        self
    }
}

impl Entry for Builder {
    fn endpoints(&self) -> Vec<Endpoint> {
        let mut endpoints = self
            .entry
            .as_ref()
            .map_or_else(Vec::new, |entry| entry.endpoints());
        endpoints.extend(self.endpoints.iter().cloned());
        endpoints
    }

    fn policies(&self) -> Policies {
        let given = self
            .entry
            .as_ref()
            .and_then(|entry| entry.policies().connect);
        let connect = given
            .into_iter()
            .chain(self.connect.iter().cloned())
            .collect::<Vec<_>>();
        if connect.is_empty() {
            return Policies::default();
        }
        Policies::default().with_connect(Arc::new(move |connection: &Connection<'_>| {
            connect.iter().all(|policy| policy.allows(connection))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::Protocol;

    struct Given;

    impl Entry for Given {
        fn endpoints(&self) -> Vec<Endpoint> {
            vec![Endpoint::new("api.example.com", 443).unwrap()]
        }

        fn policies(&self) -> Policies {
            Policies::default().with_connect(Arc::new(|connection: &Connection<'_>| {
                connection.port != 22
            }))
        }
    }

    // @behavior NW-020
    #[test]
    fn a_builder_adds_its_endpoints_and_policies_to_the_given_entry() {
        let mut net = Builder::on(Arc::new(Given));
        net.endpoint(Endpoint::new("cache.internal", 6379).unwrap())
            .connect_policy(|connection: &Connection<'_>| connection.host != "10.0.0.9");
        let hosts = net
            .endpoints()
            .into_iter()
            .map(|endpoint| endpoint.host)
            .collect::<Vec<_>>();
        let connect = net.policies().connect.expect("a connect policy");
        let allows = |host: &str, port| connect.allows(&Connection::new(host, port, Protocol::Tcp));
        assert_eq!(
            (
                hosts,
                allows("10.0.0.1", 80),
                allows("10.0.0.1", 22),
                allows("10.0.0.9", 80)
            ),
            (
                vec!["api.example.com".to_string(), "cache.internal".to_string()],
                true,
                false,
                false
            ),
            "Builder::on must answer the given entry's endpoints then its own, and refuse what \
             either's connect policy refuses"
        );
    }
}
