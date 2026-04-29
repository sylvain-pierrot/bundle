//! Streaming bundle I/O.

pub(crate) mod adapters;
#[cfg(feature = "async")]
mod async_reader;
#[cfg(feature = "async")]
mod async_writer;
mod reader;
mod writer;

pub use reader::{BlockEvent, BundleReader, OpenBundleReader, PayloadReader};
pub use writer::BundleWriter;

#[cfg(feature = "async")]
pub use async_reader::BundleAsyncReader;
#[cfg(feature = "async")]
pub use async_writer::BundleAsyncWriter;
