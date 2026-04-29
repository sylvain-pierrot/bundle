//! Streaming bundle I/O.

mod reader;
pub mod retention;
pub(crate) mod tee;
mod writer;

pub use reader::{BlockEvent, BundleReader, PayloadReader};
#[cfg(feature = "async")]
pub use retention::AsyncRetention;
pub use retention::{DiskRetention, MemoryRetention, Retention};
pub use writer::BundleWriter;
