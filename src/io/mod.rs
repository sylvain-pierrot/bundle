//! Streaming bundle I/O.

#[cfg(feature = "async")]
mod async_reader;
#[cfg(feature = "async")]
mod async_writer;
mod reader;
pub(crate) mod tee;
mod writer;

pub use reader::{BlockEvent, BundleReader, OpenBundleReader, PayloadReader};
pub use writer::BundleWriter;

#[cfg(feature = "async")]
pub use async_reader::BundleAsyncReader;
#[cfg(feature = "async")]
pub use async_writer::BundleAsyncWriter;
