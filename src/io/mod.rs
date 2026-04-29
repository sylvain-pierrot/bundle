//! Streaming bundle I/O.

#[cfg(feature = "async")]
mod async_reader;
mod reader;
pub(crate) mod tee;
mod writer;

#[cfg(feature = "async")]
pub use crate::retention::AsyncRetention;
#[cfg(feature = "async")]
pub use async_reader::BundleAsyncReader;
pub use reader::{BlockEvent, BundleReader, OpenBundleReader, PayloadReader};
pub use writer::BundleWriter;
