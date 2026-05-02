//! Streaming bundle reader with optional ingress filter pipeline.

pub(crate) mod open;
mod sync;

#[cfg(feature = "async")]
mod async_impl;

pub use open::OpenBundleReader;
pub use sync::BundleReader;

#[cfg(feature = "async")]
pub use async_impl::BundleAsyncReader;

use crate::bundle::Bundle;
use crate::filter::FilterRejection;

/// Result of reading a bundle through a filter pipeline.
pub enum ReadResult<S> {
    /// Bundle passed all filters and is ready to use.
    Accepted(Bundle<S>),
    /// Bundle was rejected by a filter.
    Rejected(FilterRejection),
}

impl<S: core::fmt::Debug> core::fmt::Debug for ReadResult<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReadResult::Accepted(b) => f.debug_tuple("Accepted").field(b).finish(),
            ReadResult::Rejected(r) => f.debug_tuple("Rejected").field(r).finish(),
        }
    }
}

/// What [`OpenBundleReader::next_block`] yielded.
pub enum BlockEvent {
    Extension(usize),
    Payload { len: u64 },
}
