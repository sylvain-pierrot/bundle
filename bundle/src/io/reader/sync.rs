//! Sync bundle reader.

use alloc::sync::Arc;

use bundle_bpv7::Error;
use bundle_io::Read;

use crate::filter::{BundleFilter, BundleMutator, FilterChain};
use crate::retention::Retention;

use super::ReadResult;
use super::open::OpenBundleReader;

/// Streaming bundle parser with optional ingress filter pipeline.
///
/// Filters run in-flight before the payload touches retention.
/// Rejected bundles waste zero storage I/O.
pub struct BundleReader {
    chain: Arc<FilterChain>,
}

impl BundleReader {
    pub fn new() -> Self {
        BundleReader {
            chain: Arc::new(FilterChain::new()),
        }
    }

    pub fn filter(mut self, f: impl BundleFilter + 'static) -> Self {
        Arc::get_mut(&mut self.chain)
            .expect("filter must be called before cloning the reader")
            .add_filter(f);
        self
    }

    pub fn mutator(mut self, m: impl BundleMutator + 'static) -> Self {
        Arc::get_mut(&mut self.chain)
            .expect("mutator must be called before cloning the reader")
            .add_mutator(m);
        self
    }

    /// Parse a bundle from a source, storing wire bytes in the retention.
    pub fn read_from<R: Read, S: Retention>(
        &self,
        source: R,
        retention: S,
    ) -> Result<ReadResult<S>, Error> {
        OpenBundleReader::open(source, retention, self.chain.clone()).into_bundle()
    }

    /// Open a source for step-by-step parsing.
    pub fn open<R: Read, S: Retention>(&self, source: R, retention: S) -> OpenBundleReader<R, S> {
        OpenBundleReader::open(source, retention, self.chain.clone())
    }
}

impl Default for BundleReader {
    fn default() -> Self {
        Self::new()
    }
}
