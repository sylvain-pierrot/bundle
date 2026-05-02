//! Streaming bundle I/O.

pub(crate) mod adapters;
#[cfg(feature = "async")]
mod async_writer;
mod reader;
mod writer;

pub use reader::{BlockEvent, BundleReader, OpenBundleReader, ReadResult};
pub use writer::{BundleWriter, OpenBundleWriter};

#[cfg(feature = "async")]
pub use async_writer::{BundleAsyncWriter, OpenBundleAsyncWriter};
#[cfg(feature = "async")]
pub use reader::BundleAsyncReader;
