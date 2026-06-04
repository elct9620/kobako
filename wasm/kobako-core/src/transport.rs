//! Kobako transport layer — guest-side mirror of the host's
//! `lib/kobako/transport/` directory. One file per value object —
//! `Request` (`request`), `Response` (`response`), `Yield` (`block`)
//! — plus the guest dispatch path (`proxy`). Each type is re-exported at
//! this root so call sites name it `transport::Request` etc., matching the
//! host's flat `Kobako::Transport::Request`.
//!
//! Each envelope type carries its own wire codec through the
//! `crate::codec::Encode` / `crate::codec::Decode` traits — the
//! Rust-native expression of the shared contract the Ruby host gets via
//! duck typing (`#encode` / `.decode` on each value object). Those traits
//! live at the codec tier because the per-run `Outcome` / `Panic` records
//! implement them too; a value object is encoded or decoded as a whole and
//! any fault surfaces through the one `crate::codec::Error` channel.

pub mod block;
pub mod proxy;
pub mod request;
pub mod response;

pub use block::{Yield, TAG_BREAK, TAG_ERROR, TAG_OK, TAG_RESERVED};
pub use request::{Request, Target};
pub use response::Response;
