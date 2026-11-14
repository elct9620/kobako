//! The ABI's fixed values — the names and limits a host and a guest must
//! already agree on before either can read the other's bytes.
//!
//! The symbols that carry the ABI are declared where they are
//! implemented: the guest contract crate emits the exports, the driver
//! resolves them and provides the imports. What lives here is what both
//! sides must spell the same way.

/// Spell an ABI name as a literal, for the attributes that accept no
/// constant: a guest's `export_name`, and its `wasm_import_module` through
/// `(host => $then)`, which hands the host interface to `$then!`.
#[doc(hidden)]
#[macro_export]
macro_rules! abi_name {
    (version) => {
        "0.6.0"
    };
    (guest) => {
        "kobako:abi/guest@0.6.0"
    };
    (host) => {
        "kobako:abi/host@0.6.0"
    };
    (host => $then:ident) => {
        $then!("kobako:abi/host@0.6.0");
    };
    (downcall) => {
        "downcall"
    };
    (downcall_export) => {
        concat!($crate::abi_name!(guest), "#", $crate::abi_name!(downcall))
    };
    (upcall) => {
        "upcall"
    };
    (preamble) => {
        "preamble"
    };
}

/// The Guest ABI version, as the semver every ABI name carries. A host
/// accepts a Guest Binary only when its names carry this one, so a framing
/// or symbol change is an increment rather than a negotiation. A new request
/// tag needs neither, but a new answer arm is an increment.
pub const ABI_VERSION: &str = abi_name!(version);

/// The interface a Guest Binary exports its downcall under.
pub const GUEST_INTERFACE: &str = abi_name!(guest);

/// The interface a Guest Binary imports the upcall and the Preamble from.
pub const HOST_INTERFACE: &str = abi_name!(host);

/// The guest function a host calls with each request.
pub const DOWNCALL: &str = abi_name!(downcall);

/// The core export the downcall stands under: its interface, `#`, then the
/// function, as the Canonical ABI's legacy core naming spells it.
pub const DOWNCALL_EXPORT: &str = abi_name!(downcall_export);

/// The host function a guest calls with each Call.
pub const UPCALL: &str = abi_name!(upcall);

/// The host function a guest calls for the Preamble of its Invocation.
pub const PREAMBLE: &str = abi_name!(preamble);

/// Whether `name` ends with `ABI_VERSION`, readable in a const context.
const fn carries_version(name: &str) -> bool {
    let (name, version) = (name.as_bytes(), ABI_VERSION.as_bytes());
    if name.len() < version.len() {
        return false;
    }
    let start = name.len() - version.len();
    let mut i = 0;
    while i < version.len() {
        if name[start + i] != version[i] {
            return false;
        }
        i += 1;
    }
    true
}

// Each interface literal spells the version again, since an attribute takes
// no constant; a bump that misses one fails the build.
const _: () = assert!(carries_version(GUEST_INTERFACE) && carries_version(HOST_INTERFACE));

/// The largest message either side sends: 16 MiB, the whole Envelope
/// rather than the payload alone. A larger one is a wire violation the
/// receiving side refuses before allocating for it.
pub const MAX_MESSAGE_LEN: usize = 16 * 1024 * 1024;
