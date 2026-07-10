//! Kobako transport envelopes — mirror of the host's
//! `lib/kobako/transport/` directory. One file per value object —
//! `Request` (`request`), `Response` (`response`), `Run` (`run`),
//! `Yield` (`block`) — each re-exported at this root so call sites name it
//! `transport::Request` etc., matching the host's flat
//! `Kobako::Transport::Request`. The guest dispatch path that drives
//! these envelopes over the ABI lives in `kobako-core`
//! (`transport::proxy` there), keeping this module wire-only.
//!
//! Each envelope type carries its own wire codec through the
//! `crate::codec::Encode` / `crate::codec::Decode` traits — the
//! Rust-native expression of the shared contract the Ruby host gets via
//! duck typing (`#encode` / `.decode` on each value object). Those traits
//! live at the codec tier because the per-run `Outcome` / `Panic` records
//! implement them too; a value object is encoded or decoded as a whole and
//! any fault surfaces through the one `crate::codec::Error` channel.

pub mod block;
pub mod request;
pub mod response;
pub mod run;

pub use block::{Yield, TAG_BREAK, TAG_ERROR, TAG_OK};
pub use request::{Request, Target};
pub use response::Response;
pub use run::Run;
