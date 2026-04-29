//! Streaming bundle I/O.
//!
//! [`BundleReader`] parses a bundle from any [`Read`](std::io::Read) source —
//! primary and extension blocks are parsed eagerly (small, always fit in
//! memory), then the payload streams through without buffering.
//!
//! [`BundleWriter`] encodes a bundle to any [`Write`](std::io::Write) sink —
//! blocks are encoded from in-memory structs, then payload data is written in
//! chunks.
//!
//! [`Retention`] abstracts the storage backend where received bundle bytes
//! are retained. Use [`Bundle::receive`](crate::Bundle::receive) to parse a
//! bundle from a stream while storing all bytes in a retention backend.

mod decode;
mod reader;
pub mod retention;
pub(crate) mod tee;

pub use reader::{BlockEvent, BundleReader, PayloadReader};
pub use retention::{MemoryRetention, Retention};
