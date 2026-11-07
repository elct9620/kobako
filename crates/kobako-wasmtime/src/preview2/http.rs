//! What the Wasm Driver answers each `wasi:http` import a client makes. A
//! request the guest builds is handed to the Invocation's namespace, which
//! sends it through the outbound seam; until a Grant gives the guest the
//! network, `handle` answers `HTTP-request-denied`, as a host that sends nothing
//! does.

use super::bindings::HttpHost;
use crate::invocation::Invocation;

impl HttpHost for Invocation {}
